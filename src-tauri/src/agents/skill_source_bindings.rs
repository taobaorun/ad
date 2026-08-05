use std::fs::File;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fsync, open, readlinkat, renameat_with, unlinkat, AtFlags, Mode, OFlags, RenameFlags,
};
use serde::{Deserialize, Serialize};

use super::skill_activation::{inspect_activation_impact, inspect_skills};
use super::skill_artifact_tree::{inspect_tree, ArtifactLimits};
use super::{
    ContentDigest, SkillActivationImpact, SkillArtifactError, SkillArtifactItem, SkillSourceType,
};
use crate::fs::paths::{
    ad_home, managed_skill_source_dir, managed_skill_source_generations_dir,
    skill_acquisition_staging_dir, skill_library_dir,
};
use crate::models::SkillSource;

pub const SKILL_SOURCE_BINDING_SCHEMA_VERSION: u32 = 2;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillSourceBinding {
    pub schema_version: u32,
    pub binding_id: String,
    pub source_id: String,
    pub source_type: SkillSourceType,
    pub source_revision: String,
    pub stable_root: String,
    pub physical_root: String,
    pub tree_digest: ContentDigest,
    pub manifest_digest: ContentDigest,
    pub skills: Vec<SkillArtifactItem>,
    pub activation_impact: SkillActivationImpact,
}

pub struct StagedGitSkillSourceBinding {
    operation_root: PathBuf,
    selected_subdirectory: Option<PathBuf>,
    generation_name: String,
    binding: SkillSourceBinding,
    _lease: File,
    published: bool,
}

impl StagedGitSkillSourceBinding {
    pub fn binding(&self) -> &SkillSourceBinding {
        &self.binding
    }
}

impl Drop for StagedGitSkillSourceBinding {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_dir_all(&self.operation_root);
        }
    }
}

#[derive(Debug)]
pub struct PublishedGitSkillSourceBinding {
    current_path: PathBuf,
    previous_target: Option<PathBuf>,
    installed_target: PathBuf,
    committed: bool,
}

impl PublishedGitSkillSourceBinding {
    pub fn commit(mut self) {
        self.committed = true;
    }

    pub fn compensate(mut self) -> Result<(), SkillArtifactError> {
        restore_current_link(
            &self.current_path,
            self.previous_target.as_deref(),
            &self.installed_target,
        )?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PublishedGitSkillSourceBinding {
    fn drop(&mut self) {
        if !self.committed {
            let _ = restore_current_link(
                &self.current_path,
                self.previous_target.as_deref(),
                &self.installed_target,
            );
        }
    }
}

impl SkillSourceBinding {
    pub fn skill_root(&self, subpath: &str) -> PathBuf {
        Path::new(&self.stable_root).join(subpath)
    }
}

pub fn resolve_skill_source_item(
    binding: &SkillSourceBinding,
    item: &SkillArtifactItem,
) -> Result<(PathBuf, PathBuf), SkillArtifactError> {
    if binding.schema_version != SKILL_SOURCE_BINDING_SCHEMA_VERSION {
        return Err(SkillArtifactError::Corrupt(format!(
            "unsupported Skill source binding schema {}",
            binding.schema_version
        )));
    }
    if binding.source_type == SkillSourceType::Git {
        validate_managed_git_binding_location(binding)?;
    }
    let stable_root = Path::new(&binding.stable_root);
    let physical_root = Path::new(&binding.physical_root);
    if !stable_root.is_absolute() || !physical_root.is_absolute() {
        return Err(SkillArtifactError::Corrupt(
            "Skill source binding roots must be absolute".into(),
        ));
    }
    let relative = if item.subpath.is_empty() {
        PathBuf::new()
    } else {
        safe_subdirectory(&item.subpath)?
    };
    let physical_root = std::fs::canonicalize(physical_root)
        .map_err(|error| io_error(Path::new(&binding.physical_root), error))?;
    let resolved_stable =
        std::fs::canonicalize(stable_root).map_err(|error| io_error(stable_root, error))?;
    if resolved_stable != physical_root {
        return Err(SkillArtifactError::Corrupt(
            "Skill source stable root no longer resolves to its physical root".into(),
        ));
    }
    let stable_skill = stable_root.join(&relative);
    let physical_skill = std::fs::canonicalize(physical_root.join(&relative))
        .map_err(|error| io_error(&physical_root.join(&relative), error))?;
    if !physical_skill.starts_with(&physical_root)
        || !physical_skill.is_dir()
        || !physical_skill.join("SKILL.md").is_file()
    {
        return Err(SkillArtifactError::Corrupt(format!(
            "Skill source item is unavailable: {}",
            item.logical_id
        )));
    }
    Ok((stable_skill, physical_skill))
}

pub fn inspect_local_skill_source_binding(
    source: &SkillSource,
) -> Result<SkillSourceBinding, SkillArtifactError> {
    if source.source_type != SkillSourceType::Local {
        return Err(SkillArtifactError::InvalidSource(
            "local source binding inspection requires a Local source".into(),
        ));
    }
    let requested_root = Path::new(&source.url);
    let canonical_root =
        std::fs::canonicalize(requested_root).map_err(|error| io_error(requested_root, error))?;
    if !canonical_root.is_dir() {
        return Err(SkillArtifactError::InvalidSource(
            "local source is not a directory".into(),
        ));
    }
    let selected_root = select_source_root(&canonical_root, source.subdirectory.as_deref())?;
    let manifest = inspect_tree(&selected_root, ArtifactLimits::default())?;
    let tree_digest = manifest.digest()?;
    let skills = inspect_skills(&selected_root, &manifest)?;
    if skills.is_empty() {
        return Err(SkillArtifactError::InvalidSource(
            "source contains no SKILL.md".into(),
        ));
    }
    let activation_impact = inspect_activation_impact(&selected_root, &manifest)?;
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
    let source_key = skill_source_key(&source.id);
    let selected_root = selected_root.to_string_lossy().into_owned();

    Ok(SkillSourceBinding {
        schema_version: SKILL_SOURCE_BINDING_SCHEMA_VERSION,
        binding_id: format!("skill-source-binding:{source_key}"),
        source_id: source.id.clone(),
        source_type: source.source_type,
        source_revision: format!("local:{}", tree_digest.as_str()),
        stable_root: selected_root.clone(),
        physical_root: selected_root,
        tree_digest,
        manifest_digest: ContentDigest::sha256(&manifest_bytes),
        skills,
        activation_impact,
    })
}

pub fn stage_git_skill_source_binding(
    source: &SkillSource,
) -> Result<StagedGitSkillSourceBinding, SkillArtifactError> {
    if source.source_type != SkillSourceType::Git {
        return Err(SkillArtifactError::InvalidSource(
            "Git source binding staging requires a Git source".into(),
        ));
    }
    let staging_parent = skill_acquisition_staging_dir().map_err(path_error)?;
    let ad_root = ad_home().map_err(path_error)?;
    ensure_private_directory(&ad_root)?;
    ensure_private_directory(&ad_root.join("staging"))?;
    ensure_private_directory(&staging_parent)?;
    let operation_root = staging_parent.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&operation_root).map_err(|error| io_error(&operation_root, error))?;
    std::fs::set_permissions(&operation_root, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| io_error(&operation_root, error))?;
    let lease = super::skill_artifact_lease::acquire_staging_lease(&operation_root)?;
    let result = (|| {
        let checkout_root = operation_root.join("source");
        crate::fs::git::clone(&source.url, &checkout_root, source.branch.as_deref())
            .map_err(|error| SkillArtifactError::Git(error.to_string()))?;
        let revision = crate::fs::git::head_revision(&checkout_root)
            .map_err(|error| SkillArtifactError::Git(error.to_string()))?;
        build_staged_git_binding(
            source,
            operation_root.clone(),
            checkout_root,
            revision,
            lease,
        )
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&operation_root);
    }
    result
}

fn build_staged_git_binding(
    source: &SkillSource,
    operation_root: PathBuf,
    checkout_root: PathBuf,
    revision: String,
    lease: File,
) -> Result<StagedGitSkillSourceBinding, SkillArtifactError> {
    let selected_root = select_source_root(&checkout_root, source.subdirectory.as_deref())?;
    let manifest = inspect_tree(&selected_root, ArtifactLimits::default())?;
    let tree_digest = manifest.digest()?;
    let skills = inspect_skills(&selected_root, &manifest)?;
    if skills.is_empty() {
        return Err(SkillArtifactError::InvalidSource(
            "source contains no SKILL.md".into(),
        ));
    }
    let activation_impact = inspect_activation_impact(&selected_root, &manifest)?;
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
    let source_key = skill_source_key(&source.id);
    let source_root = managed_skill_source_dir(&source_key).map_err(path_error)?;
    let generations = managed_skill_source_generations_dir(&source_key).map_err(path_error)?;
    let source_revision = format!("git:{revision}");
    let generation_digest =
        ContentDigest::sha256(format!("{}:{}", source_revision, tree_digest.as_str()).as_bytes());
    let generation_name = generation_digest
        .as_str()
        .strip_prefix("sha256:")
        .expect("SHA-256 digest has a stable prefix")
        .to_owned();
    let generation_root = generations.join(&generation_name);
    let selected_subdirectory = source
        .subdirectory
        .as_deref()
        .map(safe_subdirectory)
        .transpose()?;
    let physical_root = selected_subdirectory
        .as_ref()
        .map(|subdirectory| generation_root.join(subdirectory))
        .unwrap_or_else(|| generation_root.clone());
    let binding = SkillSourceBinding {
        schema_version: SKILL_SOURCE_BINDING_SCHEMA_VERSION,
        binding_id: format!("skill-source-binding:{source_key}"),
        source_id: source.id.clone(),
        source_type: source.source_type,
        source_revision,
        stable_root: source_root.join("current").to_string_lossy().into_owned(),
        physical_root: physical_root.to_string_lossy().into_owned(),
        tree_digest,
        manifest_digest: ContentDigest::sha256(&manifest_bytes),
        skills,
        activation_impact,
    };
    Ok(StagedGitSkillSourceBinding {
        operation_root,
        selected_subdirectory,
        generation_name,
        binding,
        _lease: lease,
        published: false,
    })
}

#[cfg(test)]
pub(super) fn stage_existing_git_checkout_for_test(
    source: &SkillSource,
    operation_root: PathBuf,
    revision: &str,
) -> Result<StagedGitSkillSourceBinding, SkillArtifactError> {
    let lease = super::skill_artifact_lease::acquire_staging_lease(&operation_root)?;
    build_staged_git_binding(
        source,
        operation_root.clone(),
        operation_root.join("source"),
        revision.to_owned(),
        lease,
    )
}

pub fn publish_staged_git_skill_source_binding(
    mut staged: StagedGitSkillSourceBinding,
    previous: Option<&SkillSourceBinding>,
) -> Result<(SkillSourceBinding, PublishedGitSkillSourceBinding), SkillArtifactError> {
    validate_managed_git_binding_location(&staged.binding)?;
    if let Some(previous) = previous {
        validate_managed_git_binding_location(previous)?;
    }
    let source_key = skill_source_key(&staged.binding.source_id);
    let source_root = managed_skill_source_dir(&source_key).map_err(path_error)?;
    let generations = managed_skill_source_generations_dir(&source_key).map_err(path_error)?;
    ensure_managed_git_source_directories(&source_key)?;
    let current_path = source_root.join("current");
    let previous_target = validate_current_before_switch(&current_path, previous)?;
    let generation_root = generations.join(&staged.generation_name);
    if generation_root.exists() || generation_root.is_symlink() {
        let selected = staged
            .selected_subdirectory
            .as_ref()
            .map(|subdirectory| generation_root.join(subdirectory))
            .unwrap_or_else(|| generation_root.clone());
        let manifest = inspect_tree(&selected, ArtifactLimits::default())?;
        if manifest.digest()? != staged.binding.tree_digest {
            return Err(SkillArtifactError::Corrupt(
                "existing Git source generation differs from staged content".into(),
            ));
        }
    } else {
        let operation = open(&staged.operation_root, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|error| io_error(&staged.operation_root, error.into()))?;
        let generation_parent = open(&generations, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|error| io_error(&generations, error.into()))?;
        renameat_with(
            &operation,
            "source",
            &generation_parent,
            staged.generation_name.as_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| io_error(&generation_root, error.into()))?;
        fsync(&generation_parent).map_err(|error| io_error(&generations, error.into()))?;
    }
    let mut installed_target = PathBuf::from("generations").join(&staged.generation_name);
    if let Some(subdirectory) = &staged.selected_subdirectory {
        installed_target.push(subdirectory);
    }
    replace_current_link(&current_path, &installed_target)?;
    staged.published = true;
    let _ = std::fs::remove_dir_all(&staged.operation_root);
    let published = PublishedGitSkillSourceBinding {
        current_path,
        previous_target,
        installed_target,
        committed: false,
    };
    Ok((staged.binding.clone(), published))
}

pub fn switch_git_skill_source_binding(
    target: &SkillSourceBinding,
    current: &SkillSourceBinding,
) -> Result<PublishedGitSkillSourceBinding, SkillArtifactError> {
    if target.source_type != SkillSourceType::Git
        || current.source_type != SkillSourceType::Git
        || target.source_id != current.source_id
        || target.binding_id != current.binding_id
        || target.stable_root != current.stable_root
    {
        return Err(SkillArtifactError::Corrupt(
            "Git source rollback bindings do not identify one stable source".into(),
        ));
    }
    validate_managed_git_binding_location(target)?;
    validate_managed_git_binding_location(current)?;
    let target_root = Path::new(&target.physical_root);
    let manifest = inspect_tree(target_root, ArtifactLimits::default())?;
    if manifest.digest()? != target.tree_digest {
        return Err(SkillArtifactError::Corrupt(
            "Git source rollback generation differs from its receipt".into(),
        ));
    }
    let current_path = PathBuf::from(&target.stable_root);
    let previous_target = validate_current_before_switch(&current_path, Some(current))?;
    let installed_target = relative_generation_target(&current_path, &target.physical_root)?;
    replace_current_link(&current_path, &installed_target)?;
    Ok(PublishedGitSkillSourceBinding {
        current_path,
        previous_target,
        installed_target,
        committed: false,
    })
}

pub fn reconcile_git_skill_source_current(
    after: &SkillSourceBinding,
    previous: Option<&SkillSourceBinding>,
    catalog_committed: bool,
) -> Result<(), SkillArtifactError> {
    if after.source_type != SkillSourceType::Git {
        return Ok(());
    }
    validate_managed_git_binding_location(after)?;
    if let Some(previous) = previous {
        validate_managed_git_binding_location(previous)?;
        if previous.source_id != after.source_id
            || previous.binding_id != after.binding_id
            || previous.stable_root != after.stable_root
        {
            return Err(SkillArtifactError::Corrupt(
                "Git source journal bindings do not identify one stable source".into(),
            ));
        }
    }
    let current = Path::new(&after.stable_root);
    let parent = open_current_parent(current)?;
    let current_target = match readlinkat(&parent, "current", Vec::new()) {
        Ok(target) => Some(PathBuf::from(std::ffi::OsString::from_vec(
            target.as_bytes().to_vec(),
        ))),
        Err(rustix::io::Errno::NOENT) => None,
        Err(error) => return Err(io_error(current, error.into())),
    };
    let after_target = relative_generation_target(current, &after.physical_root)?;
    if catalog_committed {
        return if current_target.as_ref() == Some(&after_target) {
            Ok(())
        } else {
            Err(SkillArtifactError::Corrupt(
                "committed Git source catalog does not match current".into(),
            ))
        };
    }
    if let Some(previous) = previous {
        let previous_target = relative_generation_target(current, &previous.physical_root)?;
        if current_target.as_ref() == Some(&previous_target) {
            return Ok(());
        }
        if current_target.as_ref() != Some(&after_target) {
            return Err(SkillArtifactError::Corrupt(
                "uncommitted Git source current differs from both journal sides".into(),
            ));
        }
        replace_current_link(current, &previous_target)
    } else if current_target.is_none() {
        Ok(())
    } else if current_target.as_ref() == Some(&after_target) {
        unlinkat(&parent, "current", AtFlags::empty())
            .and_then(|_| fsync(&parent))
            .map_err(|error| io_error(current, error.into()))
    } else {
        Err(SkillArtifactError::Corrupt(
            "uncommitted Git source add has an unexpected current link".into(),
        ))
    }
}

pub fn skill_source_key(source_id: &str) -> String {
    ContentDigest::sha256(source_id.as_bytes())
        .as_str()
        .strip_prefix("sha256:")
        .expect("ContentDigest::sha256 always returns a sha256 digest")
        .to_owned()
}

fn validate_managed_git_binding_location(
    binding: &SkillSourceBinding,
) -> Result<(), SkillArtifactError> {
    if binding.schema_version != SKILL_SOURCE_BINDING_SCHEMA_VERSION
        || binding.source_type != SkillSourceType::Git
    {
        return Err(SkillArtifactError::Corrupt(
            "managed Git source binding has invalid identity metadata".into(),
        ));
    }
    let source_key = skill_source_key(&binding.source_id);
    let source_root = managed_skill_source_dir(&source_key).map_err(path_error)?;
    let expected_binding_id = format!("skill-source-binding:{source_key}");
    if binding.binding_id != expected_binding_id
        || Path::new(&binding.stable_root) != source_root.join("current")
    {
        return Err(SkillArtifactError::Corrupt(
            "managed Git source binding is outside its backend-derived source root".into(),
        ));
    }
    relative_generation_target(&source_root.join("current"), binding.physical_root.as_str())?;
    Ok(())
}

fn select_source_root(
    root: &Path,
    subdirectory: Option<&str>,
) -> Result<PathBuf, SkillArtifactError> {
    let Some(subdirectory) = subdirectory else {
        return Ok(root.to_path_buf());
    };
    let relative = safe_subdirectory(subdirectory)?;
    let requested = root.join(relative);
    let selected =
        std::fs::canonicalize(&requested).map_err(|error| io_error(&requested, error))?;
    if !selected.is_dir() || !selected.starts_with(root) {
        return Err(SkillArtifactError::InvalidSource(
            "Skill source subdirectory escapes its source root".into(),
        ));
    }
    Ok(selected)
}

fn safe_subdirectory(value: &str) -> Result<PathBuf, SkillArtifactError> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SkillArtifactError::InvalidSource(
            "invalid Skill source subdirectory".into(),
        ));
    }
    Ok(path.to_path_buf())
}

fn io_error(path: &Path, source: std::io::Error) -> SkillArtifactError {
    SkillArtifactError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn ensure_managed_git_source_directories(source_key: &str) -> Result<(), SkillArtifactError> {
    let ad_root = ad_home().map_err(path_error)?;
    ensure_private_directory(&ad_root)?;
    let library = skill_library_dir().map_err(path_error)?;
    ensure_private_directory(&library)?;
    let source_root = managed_skill_source_dir(source_key).map_err(path_error)?;
    ensure_private_directory(&source_root)?;
    let generations = managed_skill_source_generations_dir(source_key).map_err(path_error)?;
    ensure_private_directory(&generations)
}

fn ensure_private_directory(path: &Path) -> Result<(), SkillArtifactError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(SkillArtifactError::InvalidSource(format!(
                "managed path is not a physical directory: {}",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|error| io_error(path, error))?;
        }
        Err(error) => return Err(io_error(path, error)),
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| io_error(path, error))
}

fn validate_current_before_switch(
    current: &Path,
    previous: Option<&SkillSourceBinding>,
) -> Result<Option<PathBuf>, SkillArtifactError> {
    let parent = open_current_parent(current)?;
    match readlinkat(&parent, "current", Vec::new()) {
        Ok(target) => {
            let target = PathBuf::from(std::ffi::OsString::from_vec(target.as_bytes().to_vec()));
            let previous = previous.ok_or_else(|| {
                SkillArtifactError::Corrupt(
                    "managed Git source current link exists without catalog provenance".into(),
                )
            })?;
            let expected = relative_generation_target(current, &previous.physical_root)?;
            if target != expected {
                return Err(SkillArtifactError::Corrupt(
                    "managed Git source current link changed after preview".into(),
                ));
            }
            Ok(Some(target))
        }
        Err(rustix::io::Errno::NOENT) if previous.is_none() => Ok(None),
        Err(rustix::io::Errno::NOENT) => Err(SkillArtifactError::Corrupt(
            "managed Git source current link is missing".into(),
        )),
        Err(rustix::io::Errno::INVAL) => Err(SkillArtifactError::Corrupt(
            "managed Git source current is not a symlink".into(),
        )),
        Err(error) => Err(io_error(current, error.into())),
    }
}

fn replace_current_link(current: &Path, target: &Path) -> Result<(), SkillArtifactError> {
    let parent = open_current_parent(current)?;
    super::execution_tree::write_symlink_atomic(&parent, std::ffi::OsStr::new("current"), target)
        .map_err(|error| io_error(current, error))
}

fn restore_current_link(
    current: &Path,
    previous_target: Option<&Path>,
    installed_target: &Path,
) -> Result<(), SkillArtifactError> {
    let parent = open_current_parent(current)?;
    let actual = readlinkat(&parent, "current", Vec::new())
        .map(|target| PathBuf::from(std::ffi::OsString::from_vec(target.as_bytes().to_vec())))
        .map_err(|error| io_error(current, error.into()))?;
    if actual != installed_target {
        return Err(SkillArtifactError::Corrupt(
            "managed Git source current link changed before compensation".into(),
        ));
    }
    if let Some(previous_target) = previous_target {
        replace_current_link(current, previous_target)
    } else {
        unlinkat(&parent, "current", AtFlags::empty())
            .and_then(|_| fsync(&parent))
            .map_err(|error| io_error(current, error.into()))
    }
}

fn open_current_parent(current: &Path) -> Result<OwnedFd, SkillArtifactError> {
    let parent = current.parent().ok_or_else(|| {
        SkillArtifactError::Corrupt("managed Git source current link has no parent".into())
    })?;
    open(parent, DIRECTORY_FLAGS, Mode::empty()).map_err(|error| io_error(parent, error.into()))
}

fn relative_generation_target(
    current: &Path,
    physical_root: &str,
) -> Result<PathBuf, SkillArtifactError> {
    let source_root = current.parent().ok_or_else(|| {
        SkillArtifactError::Corrupt("managed Git source current link has no parent".into())
    })?;
    let relative = Path::new(physical_root)
        .strip_prefix(source_root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            SkillArtifactError::Corrupt(
                "Git source generation is outside its managed source root".into(),
            )
        })?;
    let mut components = relative.components();
    if components.next() != Some(Component::Normal(std::ffi::OsStr::new("generations")))
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SkillArtifactError::Corrupt(
            "Git source generation has an invalid managed path".into(),
        ));
    }
    Ok(relative)
}

fn path_error(error: crate::fs::FsError) -> SkillArtifactError {
    SkillArtifactError::InvalidSource(error.to_string())
}

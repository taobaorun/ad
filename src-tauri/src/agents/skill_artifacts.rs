use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{open, renameat_with, Mode, OFlags, RenameFlags};
use serde::{Deserialize, Serialize};

use super::skill_activation::{inspect_activation_impact, inspect_skills};
use super::skill_artifact_tree::{
    copy_tree_verified, inspect_tree, ArtifactLimits, ArtifactTreeError, TreeEntryKind,
    TreeManifest,
};
use super::ContentDigest;
use crate::fs::paths::{skill_acquisition_staging_dir, skill_artifacts_dir};
use crate::models::{SkillSource, SkillSourceType};

const ARTIFACT_SCHEMA_VERSION: u32 = 1;
const MIN_FREE_SPACE_BYTES: u64 = 128 * 1024 * 1024;
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillArtifactRef {
    pub schema_version: u32,
    pub artifact_id: String,
    pub source_id: String,
    pub source_revision: String,
    pub tree_digest: ContentDigest,
    pub manifest_digest: ContentDigest,
    pub skills: Vec<SkillArtifactItem>,
    pub activation_impact: SkillActivationImpact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillArtifactItem {
    pub logical_id: String,
    pub subpath: String,
    pub instruction_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillActivationImpact {
    pub instructions: Vec<String>,
    pub hooks: Vec<String>,
    pub mcp: Vec<String>,
    pub commands: Vec<String>,
    pub scripts: Vec<String>,
    pub binaries: Vec<String>,
    pub executable_paths: Vec<String>,
    pub digest: ContentDigest,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillArtifactError {
    #[error("invalid Skill source: {0}")]
    InvalidSource(String),
    #[error("Skill artifact is corrupt: {0}")]
    Corrupt(String),
    #[error("insufficient free disk space for Skill acquisition")]
    InsufficientDisk,
    #[error("Skill artifact tree validation failed: {0}")]
    Tree(String),
    #[error("Skill artifact I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Skill source Git operation failed: {0}")]
    Git(String),
}

impl From<ArtifactTreeError> for SkillArtifactError {
    fn from(error: ArtifactTreeError) -> Self {
        Self::Tree(error.to_string())
    }
}

pub struct StagedSkillArtifact {
    operation_root: PathBuf,
    artifact_root: PathBuf,
    manifest: StoredArtifactManifest,
    reference: SkillArtifactRef,
    _lease: File,
    published: bool,
}

impl StagedSkillArtifact {
    pub fn reference(&self) -> &SkillArtifactRef {
        &self.reference
    }

    pub fn operation_id(&self) -> Option<&str> {
        self.operation_root.file_name()?.to_str()
    }
}

impl Drop for StagedSkillArtifact {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_dir_all(&self.operation_root);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredArtifactManifest {
    schema_version: u32,
    tree_digest: ContentDigest,
    tree: TreeManifest,
    skills: Vec<SkillArtifactItem>,
    activation_impact: SkillActivationImpact,
}

pub fn stage_skill_source(source: &SkillSource) -> Result<StagedSkillArtifact, SkillArtifactError> {
    let limits = ArtifactLimits::default();
    let staging_parent = skill_acquisition_staging_dir().map_err(path_error)?;
    create_private_directory_all(&staging_parent)?;
    ensure_disk_budget(&staging_parent, limits.max_total_bytes)?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let operation_root = staging_parent.join(&operation_id);
    std::fs::create_dir(&operation_root).map_err(|source| io_error(&operation_root, source))?;
    std::fs::set_permissions(&operation_root, std::fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error(&operation_root, source))?;
    let lease = super::skill_artifact_lease::acquire_staging_lease(&operation_root)?;

    let result = (|| {
        let (source_root, mut source_revision) = materialize_source(source, &operation_root)?;
        let selected_root = select_source_root(&source_root, source.subdirectory.as_deref())?;
        let source_manifest = inspect_tree(&selected_root, limits)?;
        let artifact_root = operation_root.join("artifact");
        std::fs::create_dir(&artifact_root).map_err(|source| io_error(&artifact_root, source))?;
        let tree_root = artifact_root.join("tree");
        copy_tree_verified(&selected_root, &tree_root, &source_manifest, limits)?;
        let tree_digest = source_manifest.digest()?;
        if source.source_type == SkillSourceType::Local {
            source_revision = format!("local:{}", tree_digest.as_str());
        }
        let skills = inspect_skills(&tree_root, &source_manifest)?;
        if skills.is_empty() {
            return Err(SkillArtifactError::InvalidSource(
                "source contains no SKILL.md".into(),
            ));
        }
        let activation_impact = inspect_activation_impact(&tree_root, &source_manifest)?;
        let manifest = StoredArtifactManifest {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            tree_digest: tree_digest.clone(),
            tree: source_manifest,
            skills: skills.clone(),
            activation_impact: activation_impact.clone(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
        let manifest_digest = ContentDigest::sha256(&manifest_bytes);
        let manifest_path = artifact_root.join("manifest.json");
        std::fs::write(&manifest_path, &manifest_bytes)
            .map_err(|source| io_error(&manifest_path, source))?;
        File::open(&manifest_path)
            .and_then(|file| file.sync_all())
            .map_err(|source| io_error(&manifest_path, source))?;
        File::open(&artifact_root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| io_error(&artifact_root, source))?;
        let artifact_id = format!("skill-artifact:{}", tree_digest.as_str());
        let reference = SkillArtifactRef {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            artifact_id,
            source_id: source.id.clone(),
            source_revision,
            tree_digest,
            manifest_digest,
            skills,
            activation_impact,
        };
        Ok(StagedSkillArtifact {
            operation_root: operation_root.clone(),
            artifact_root,
            manifest,
            reference,
            _lease: lease,
            published: false,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&operation_root);
    }
    result
}

pub fn observe_skill_source_revision(source: &SkillSource) -> Result<String, SkillArtifactError> {
    match source.source_type {
        SkillSourceType::Local => {
            let root = Path::new(&source.url);
            let canonical = std::fs::canonicalize(root).map_err(|source| io_error(root, source))?;
            if !canonical.is_dir() {
                return Err(SkillArtifactError::InvalidSource(
                    "local source is not a directory".into(),
                ));
            }
            let selected = select_source_root(&canonical, source.subdirectory.as_deref())?;
            let manifest = super::resource_scanner::inspect_catalog_tree(&selected)?;
            Ok(format!("local:{}", manifest.digest()?.as_str()))
        }
        SkillSourceType::Git => {
            crate::fs::git::resolve_remote_revision(&source.url, source.branch.as_deref())
                .map(|revision| format!("git:{revision}"))
                .map_err(|error| SkillArtifactError::Git(error.to_string()))
        }
    }
}

pub fn publish_staged_skill_artifact(
    mut staged: StagedSkillArtifact,
) -> Result<SkillArtifactRef, SkillArtifactError> {
    verify_staged_artifact(&staged)?;
    let artifacts = skill_artifacts_dir().map_err(path_error)?;
    create_private_directory_all(&artifacts)?;
    let artifact_name = artifact_directory_name(&staged.reference.tree_digest)?;
    let final_root = artifacts.join(&artifact_name);
    if final_root.exists() || final_root.is_symlink() {
        verify_published_artifact(&final_root, &staged.reference, &staged.manifest)?;
        staged.published = true;
        let _ = std::fs::remove_dir_all(&staged.operation_root);
        return Ok(staged.reference.clone());
    }
    make_artifact_read_only(&staged.artifact_root, &staged.manifest)?;
    let operation_fd = open(&staged.operation_root, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|source| io_error(&staged.operation_root, source.into()))?;
    let artifacts_fd = open(&artifacts, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|source| io_error(&artifacts, source.into()))?;
    renameat_with(
        &operation_fd,
        "artifact",
        &artifacts_fd,
        artifact_name.as_str(),
        RenameFlags::NOREPLACE,
    )
    .map_err(|source| {
        if final_root.exists() {
            SkillArtifactError::Corrupt(format!(
                "artifact publication collided at {}",
                staged.reference.artifact_id
            ))
        } else {
            io_error(&final_root, source.into())
        }
    })?;
    rustix::fs::fsync(&artifacts_fd).map_err(|source| io_error(&artifacts, source.into()))?;
    verify_published_artifact(&final_root, &staged.reference, &staged.manifest)?;
    staged.published = true;
    let _ = std::fs::remove_dir_all(&staged.operation_root);
    Ok(staged.reference.clone())
}

pub fn verify_skill_artifact(reference: &SkillArtifactRef) -> Result<PathBuf, SkillArtifactError> {
    let artifacts = skill_artifacts_dir().map_err(path_error)?;
    let root = artifacts.join(artifact_directory_name(&reference.tree_digest)?);
    let bytes = std::fs::read(root.join("manifest.json"))
        .map_err(|source| io_error(&root.join("manifest.json"), source))?;
    if ContentDigest::sha256(&bytes) != reference.manifest_digest {
        return Err(SkillArtifactError::Corrupt(
            "published manifest digest changed".into(),
        ));
    }
    let manifest: StoredArtifactManifest = serde_json::from_slice(&bytes)
        .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
    verify_published_artifact(&root, reference, &manifest)?;
    Ok(root.join("tree"))
}

pub fn cleanup_unpublished_skill_staging(
    preserved_operation_ids: &BTreeSet<String>,
) -> Result<usize, SkillArtifactError> {
    let root = skill_acquisition_staging_dir().map_err(path_error)?;
    if !root.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(&root).map_err(|source| io_error(&root, source))? {
        let entry = entry.map_err(|source| io_error(&root, source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|source| io_error(&entry.path(), source))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || uuid::Uuid::parse_str(&name).is_err()
            || preserved_operation_ids.contains(&name)
        {
            continue;
        }
        let Some(_lease) = super::skill_artifact_lease::acquire_cleanup_lease(&entry.path())?
        else {
            continue;
        };
        std::fs::remove_dir_all(entry.path()).map_err(|source| io_error(&entry.path(), source))?;
        removed += 1;
    }
    Ok(removed)
}

fn materialize_source(
    source: &SkillSource,
    operation_root: &Path,
) -> Result<(PathBuf, String), SkillArtifactError> {
    match source.source_type {
        SkillSourceType::Local => {
            let path = Path::new(&source.url);
            let canonical = std::fs::canonicalize(path).map_err(|source| io_error(path, source))?;
            if !canonical.is_dir() {
                return Err(SkillArtifactError::InvalidSource(
                    "local source is not a directory".into(),
                ));
            }
            Ok((canonical, "local-pending-tree-digest".into()))
        }
        SkillSourceType::Git => {
            let checkout = operation_root.join("source");
            crate::fs::git::clone(&source.url, &checkout, source.branch.as_deref())
                .map_err(|error| SkillArtifactError::Git(error.to_string()))?;
            let revision = crate::fs::git::head_revision(&checkout)
                .map_err(|error| SkillArtifactError::Git(error.to_string()))?;
            Ok((checkout, format!("git:{revision}")))
        }
    }
}

fn select_source_root(
    root: &Path,
    subdirectory: Option<&str>,
) -> Result<PathBuf, SkillArtifactError> {
    let Some(subdirectory) = subdirectory else {
        return Ok(root.to_path_buf());
    };
    let relative = safe_subdirectory(subdirectory)?;
    let selected = std::fs::canonicalize(root.join(relative))
        .map_err(|source| io_error(&root.join(subdirectory), source))?;
    let canonical_root = std::fs::canonicalize(root).map_err(|source| io_error(root, source))?;
    if !selected.is_dir() || !selected.starts_with(&canonical_root) {
        return Err(SkillArtifactError::InvalidSource(
            "Skill source subdirectory escapes its source root".into(),
        ));
    }
    Ok(selected)
}

fn verify_staged_artifact(staged: &StagedSkillArtifact) -> Result<(), SkillArtifactError> {
    let tree = inspect_tree(
        &staged.artifact_root.join("tree"),
        ArtifactLimits::default(),
    )?;
    if tree != staged.manifest.tree || tree.digest()? != staged.reference.tree_digest {
        return Err(SkillArtifactError::Corrupt(
            "staged tree changed after acquisition".into(),
        ));
    }
    let bytes = std::fs::read(staged.artifact_root.join("manifest.json"))
        .map_err(|source| io_error(&staged.artifact_root.join("manifest.json"), source))?;
    if ContentDigest::sha256(&bytes) != staged.reference.manifest_digest {
        return Err(SkillArtifactError::Corrupt(
            "staged manifest changed after acquisition".into(),
        ));
    }
    Ok(())
}

fn verify_published_artifact(
    root: &Path,
    reference: &SkillArtifactRef,
    expected_manifest: &StoredArtifactManifest,
) -> Result<(), SkillArtifactError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|source| io_error(root, source))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SkillArtifactError::Corrupt(
            "artifact root is not a physical directory".into(),
        ));
    }
    let bytes = std::fs::read(root.join("manifest.json"))
        .map_err(|source| io_error(&root.join("manifest.json"), source))?;
    if ContentDigest::sha256(&bytes) != reference.manifest_digest {
        return Err(SkillArtifactError::Corrupt(
            "artifact manifest digest differs".into(),
        ));
    }
    let manifest: StoredArtifactManifest = serde_json::from_slice(&bytes)
        .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
    if &manifest != expected_manifest
        || manifest.tree_digest != reference.tree_digest
        || manifest.skills != reference.skills
        || manifest.activation_impact != reference.activation_impact
    {
        return Err(SkillArtifactError::Corrupt(
            "artifact manifest identity differs".into(),
        ));
    }
    let tree = inspect_tree(&root.join("tree"), ArtifactLimits::default())?;
    if tree != manifest.tree || tree.digest()? != reference.tree_digest {
        return Err(SkillArtifactError::Corrupt(
            "artifact tree differs from its manifest".into(),
        ));
    }
    Ok(())
}

fn make_artifact_read_only(
    artifact_root: &Path,
    manifest: &StoredArtifactManifest,
) -> Result<(), SkillArtifactError> {
    for entry in manifest.tree.entries.iter().rev() {
        let path = artifact_root.join("tree").join(&entry.path);
        let mode = match entry.kind {
            TreeEntryKind::Directory => 0o555,
            TreeEntryKind::File if entry.mode & 0o111 != 0 => 0o555,
            TreeEntryKind::File => 0o444,
            TreeEntryKind::Symlink => continue,
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .map_err(|source| io_error(&path, source))?;
    }
    std::fs::set_permissions(
        artifact_root.join("tree"),
        std::fs::Permissions::from_mode(0o555),
    )
    .map_err(|source| io_error(&artifact_root.join("tree"), source))?;
    std::fs::set_permissions(
        artifact_root.join("manifest.json"),
        std::fs::Permissions::from_mode(0o444),
    )
    .map_err(|source| io_error(&artifact_root.join("manifest.json"), source))?;
    Ok(())
}

fn artifact_directory_name(digest: &ContentDigest) -> Result<String, SkillArtifactError> {
    let value = digest
        .as_str()
        .strip_prefix("sha256:")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| SkillArtifactError::Corrupt("invalid tree digest".into()))?;
    Ok(value.to_owned())
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

fn ensure_disk_budget(path: &Path, max_artifact_bytes: u64) -> Result<(), SkillArtifactError> {
    let stats = rustix::fs::statvfs(path).map_err(|source| io_error(path, source.into()))?;
    let available = stats.f_bavail.saturating_mul(stats.f_frsize);
    if available < MIN_FREE_SPACE_BYTES.saturating_add(max_artifact_bytes) {
        Err(SkillArtifactError::InsufficientDisk)
    } else {
        Ok(())
    }
}

fn create_private_directory_all(path: &Path) -> Result<(), SkillArtifactError> {
    std::fs::create_dir_all(path).map_err(|source| io_error(path, source))?;
    let metadata = std::fs::symlink_metadata(path).map_err(|source| io_error(path, source))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SkillArtifactError::InvalidSource(format!(
            "managed path is not a physical directory: {}",
            path.display()
        )));
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error(path, source))
}

fn path_error(error: crate::fs::FsError) -> SkillArtifactError {
    SkillArtifactError::InvalidSource(error.to_string())
}

fn io_error(path: &Path, source: std::io::Error) -> SkillArtifactError {
    SkillArtifactError::Io {
        path: path.display().to_string(),
        source,
    }
}

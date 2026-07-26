use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs::paths::{home, project_codex_runtimes_dir};

mod migration;

use migration::ensure_runtime_name_available;
pub(crate) use migration::{
    discover_project_codex_candidates, project_runtime_descriptor_for_context,
    runtime_for_installation,
};
pub use migration::{
    persist_project_codex_runtime, project_runtime_descriptor_for_base_project,
    project_runtime_for_base_project,
};

use super::project_codex_manifest::load_project_codex_runtime_manifest;
use super::{AgentInstallation, ContentDigest, InstallationId};

const PROJECT_CODEX_RUNTIME_MANIFEST_RELATIVE_PATH: &str = ".ad/runtime-manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedAuthBinding {
    FileSymlink { source: PathBuf, target: PathBuf },
    KeychainRequiresSharedHome,
    MissingBaseLogin,
}

impl SharedAuthBinding {
    pub fn detect(
        base_home: &Path,
        runtime_home: &Path,
        credential_store: Option<&str>,
    ) -> Result<Self, ProjectCodexRuntimeError> {
        if credential_store == Some("keyring") {
            return Ok(Self::KeychainRequiresSharedHome);
        }

        let source = base_home.join("auth.json");
        match fs::metadata(&source) {
            Ok(metadata) if metadata.is_file() => Ok(Self::FileSymlink {
                source,
                target: runtime_home.join("auth.json"),
            }),
            Ok(_) => Err(ProjectCodexRuntimeError::InvalidAuthSource(
                source.to_string_lossy().into_owned(),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if credential_store == Some("file") {
                    Ok(Self::MissingBaseLogin)
                } else {
                    Ok(Self::KeychainRequiresSharedHome)
                }
            }
            Err(source_error) => Err(ProjectCodexRuntimeError::Io {
                path: source,
                source: source_error,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCodexRuntime {
    pub project_id: String,
    pub project_path: String,
    pub base_installation_id: InstallationId,
    pub runtime_installation_id: InstallationId,
    pub runtime_home: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_config_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_config_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default = "default_true")]
    pub applied_inherit_base_config: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_digest: Option<ContentDigest>,
}

pub type ProjectCodexRuntimeDescriptor = ProjectCodexRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCodexAuthMode {
    SharedFile,
    KeychainBlocked,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCodexRuntimeStatus {
    pub base_installation_id: InstallationId,
    pub runtime_installation_id: InstallationId,
    pub runtime_home: String,
    pub prepared: bool,
    pub fresh: bool,
    pub desired_inherit_base_config: bool,
    pub applied_inherit_base_config: bool,
    pub needs_refresh: bool,
    pub plugin_count: usize,
    pub auth_mode: ProjectCodexAuthMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

impl ProjectCodexRuntime {
    pub fn derive(
        base_installation: &AgentInstallation,
        project_path: &Path,
    ) -> Result<Self, ProjectCodexRuntimeError> {
        if base_installation.agent_id.as_str() != "codex" {
            return Err(ProjectCodexRuntimeError::InvalidBaseInstallation(
                base_installation.id.to_string(),
            ));
        }
        let canonical_project =
            fs::canonicalize(project_path).map_err(|source| ProjectCodexRuntimeError::Io {
                path: project_path.to_path_buf(),
                source,
            })?;
        if !canonical_project.is_dir() {
            return Err(ProjectCodexRuntimeError::InvalidProject(
                canonical_project.to_string_lossy().into_owned(),
            ));
        }

        let project_path = canonical_project.to_string_lossy().into_owned();
        let project_id = project_runtime_id(&canonical_project)?;
        let canonical_home = canonical_user_home()?;
        let runtime_home = canonical_home.join(".ad/codex-homes").join(&project_id);
        let existing = ensure_runtime_name_available(&canonical_project, &project_id)?;
        let canonical_installation_id = canonical_runtime_installation_id(&runtime_home);
        let default_base_home = canonical_home.join(".codex");
        let base_is_default = fs::canonicalize(default_base_home).ok().as_deref()
            == Some(Path::new(&base_installation.root_path));
        let base_changed = existing
            .as_ref()
            .is_some_and(|runtime| runtime.base_installation_id != base_installation.id);
        let runtime_installation_id = if base_is_default && !base_changed {
            canonical_installation_id
        } else {
            scoped_runtime_installation_id(&runtime_home, &base_installation.id)
        };

        Ok(Self {
            project_id,
            project_path,
            base_installation_id: base_installation.id.clone(),
            runtime_installation_id,
            runtime_home,
            base_config_digest: None,
            generated_config_digest: None,
            profile_id: None,
            applied_inherit_base_config: true,
            manifest_digest: None,
        })
    }

    pub fn state_path(&self) -> Result<PathBuf, ProjectCodexRuntimeError> {
        Ok(project_codex_runtimes_dir()
            .map_err(|error| ProjectCodexRuntimeError::Path(error.to_string()))?
            .join(format!("{}.json", self.project_id)))
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.runtime_home
            .join(PROJECT_CODEX_RUNTIME_MANIFEST_RELATIVE_PATH)
    }
}

pub fn refresh_project_codex_runtime_digests(
    installation_id: &InstallationId,
) -> Result<Option<ProjectCodexRuntime>, ProjectCodexRuntimeError> {
    refresh_project_codex_runtime_state(installation_id, true)
}

pub(crate) fn refresh_project_codex_runtime_after_apply(
    installation_id: &InstallationId,
    refresh_base_config: bool,
) -> Result<Option<ProjectCodexRuntime>, ProjectCodexRuntimeError> {
    refresh_project_codex_runtime_state(installation_id, refresh_base_config)
}

fn refresh_project_codex_runtime_state(
    installation_id: &InstallationId,
    refresh_base_config: bool,
) -> Result<Option<ProjectCodexRuntime>, ProjectCodexRuntimeError> {
    let Some(mut runtime) = runtime_for_installation(installation_id) else {
        return Ok(None);
    };
    let base_home = super::codex::discover_codex_candidates()
        .into_iter()
        .find(|candidate| candidate.installation().id == runtime.base_installation_id)
        .map(|candidate| PathBuf::from(&candidate.installation().root_path))
        .ok_or_else(|| {
            ProjectCodexRuntimeError::InvalidBaseInstallation(
                runtime.base_installation_id.to_string(),
            )
        })?;
    if refresh_base_config {
        runtime.base_config_digest = optional_file_digest(&base_home.join("config.toml"))?;
    }
    runtime.generated_config_digest =
        optional_file_digest(&runtime.runtime_home.join("config.toml"))?;
    match load_project_codex_runtime_manifest(&runtime)? {
        Some(snapshot) => {
            runtime.applied_inherit_base_config = snapshot.manifest.applied_inherit_base_config;
            runtime.profile_id = snapshot.manifest.applied_profile_id;
            runtime.manifest_digest = Some(snapshot.digest);
        }
        None => {
            runtime.applied_inherit_base_config = true;
            runtime.manifest_digest = None;
        }
    }
    persist_project_codex_runtime(&runtime)?;
    Ok(Some(runtime))
}

pub fn project_codex_runtime_is_fresh(
    runtime: &ProjectCodexRuntime,
) -> Result<bool, ProjectCodexRuntimeError> {
    let base_home = super::codex::discover_codex_candidates()
        .into_iter()
        .find(|candidate| candidate.installation().id == runtime.base_installation_id)
        .map(|candidate| PathBuf::from(&candidate.installation().root_path))
        .ok_or_else(|| {
            ProjectCodexRuntimeError::InvalidBaseInstallation(
                runtime.base_installation_id.to_string(),
            )
        })?;
    let base = optional_file_digest(&base_home.join("config.toml"))?;
    let generated = optional_file_digest(&runtime.runtime_home.join("config.toml"))?;
    let manifest = load_project_codex_runtime_manifest(runtime)?;
    let manifest_fresh = match &runtime.manifest_digest {
        Some(expected) => manifest.as_ref().map(|snapshot| &snapshot.digest) == Some(expected),
        None => manifest.is_none(),
    };
    let base_fresh = !runtime.applied_inherit_base_config || runtime.base_config_digest == base;
    Ok(runtime.generated_config_digest.is_some()
        && base_fresh
        && runtime.generated_config_digest == generated
        && manifest_fresh)
}

pub fn inspect_project_codex_runtime_status(
    runtime: &ProjectCodexRuntime,
    desired_inherit_base_config: bool,
) -> Result<ProjectCodexRuntimeStatus, ProjectCodexRuntimeError> {
    let base_home = super::codex::discover_codex_candidates()
        .into_iter()
        .find(|candidate| candidate.installation().id == runtime.base_installation_id)
        .map(|candidate| PathBuf::from(&candidate.installation().root_path))
        .ok_or_else(|| {
            ProjectCodexRuntimeError::InvalidBaseInstallation(
                runtime.base_installation_id.to_string(),
            )
        })?;
    let generated_path = runtime.runtime_home.join("config.toml");
    let generated = fs::read(&generated_path).ok();
    let plugin_count = generated
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| text.parse::<toml::Value>().ok())
        .and_then(|config| {
            config
                .get("plugins")
                .and_then(toml::Value::as_table)
                .map(|plugins| {
                    plugins
                        .values()
                        .filter(|config| {
                            config.get("enabled").and_then(toml::Value::as_bool) != Some(false)
                        })
                        .count()
                })
        })
        .unwrap_or(0);
    let auth_mode = match fs::symlink_metadata(runtime.runtime_home.join("auth.json")) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let expected = base_home.join("auth.json");
            if fs::read_link(runtime.runtime_home.join("auth.json"))
                .ok()
                .as_deref()
                == Some(expected.as_path())
            {
                ProjectCodexAuthMode::SharedFile
            } else {
                ProjectCodexAuthMode::Missing
            }
        }
        _ => {
            let credential_store = fs::read_to_string(base_home.join("config.toml"))
                .ok()
                .and_then(|text| text.parse::<toml::Value>().ok())
                .and_then(|config| {
                    config
                        .get("cli_auth_credentials_store")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned)
                });
            if credential_store.as_deref() == Some("keyring") {
                ProjectCodexAuthMode::KeychainBlocked
            } else {
                ProjectCodexAuthMode::Missing
            }
        }
    };
    let prepared = generated.is_some();
    let policy_matches = desired_inherit_base_config == runtime.applied_inherit_base_config;
    let fresh = project_codex_runtime_is_fresh(runtime)? && policy_matches;
    Ok(ProjectCodexRuntimeStatus {
        base_installation_id: runtime.base_installation_id.clone(),
        runtime_installation_id: runtime.runtime_installation_id.clone(),
        runtime_home: runtime.runtime_home.to_string_lossy().into_owned(),
        prepared,
        fresh,
        desired_inherit_base_config,
        applied_inherit_base_config: runtime.applied_inherit_base_config,
        needs_refresh: !prepared || !fresh,
        plugin_count,
        auth_mode,
        profile_id: runtime.profile_id.clone(),
    })
}

fn optional_file_digest(path: &Path) -> Result<Option<ContentDigest>, ProjectCodexRuntimeError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(ContentDigest::sha256(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ProjectCodexRuntimeError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn validate_runtime(
    runtime: &ProjectCodexRuntime,
) -> Result<(), ProjectCodexRuntimeError> {
    let expected_id = project_runtime_id(Path::new(&runtime.project_path))?;
    if runtime.project_id != expected_id {
        return Err(ProjectCodexRuntimeError::InvalidProjectId);
    }
    let canonical_project =
        fs::canonicalize(&runtime.project_path).map_err(|source| ProjectCodexRuntimeError::Io {
            path: PathBuf::from(&runtime.project_path),
            source,
        })?;
    if !canonical_project.is_dir()
        || canonical_project.to_string_lossy() != runtime.project_path.as_str()
    {
        return Err(ProjectCodexRuntimeError::InvalidProject(
            runtime.project_path.clone(),
        ));
    }
    let expected_home = canonical_user_home()?
        .join(".ad/codex-homes")
        .join(&runtime.project_id);
    if runtime.runtime_home != expected_home {
        return Err(ProjectCodexRuntimeError::InvalidRuntimeHome);
    }
    let expected_installation_id = canonical_runtime_installation_id(&runtime.runtime_home);
    if runtime.runtime_installation_id != expected_installation_id {
        return Err(ProjectCodexRuntimeError::InvalidRuntimeInstallationId);
    }
    Ok(())
}

pub(super) fn project_runtime_id(project_path: &Path) -> Result<String, ProjectCodexRuntimeError> {
    project_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            ProjectCodexRuntimeError::InvalidProject(project_path.to_string_lossy().into_owned())
        })
}

fn canonical_runtime_installation_id(runtime_home: &Path) -> InstallationId {
    InstallationId::from(format!("codex:{}", runtime_home.to_string_lossy()))
}

fn scoped_runtime_installation_id(
    runtime_home: &Path,
    base_installation_id: &InstallationId,
) -> InstallationId {
    InstallationId::from(format!(
        "{}::base::{}",
        canonical_runtime_installation_id(runtime_home),
        base_installation_id
    ))
}

fn scoped_base_installation_id(
    runtime_home: &Path,
    installation_id: &InstallationId,
) -> Option<InstallationId> {
    installation_id
        .as_str()
        .strip_prefix(&format!(
            "{}::base::",
            canonical_runtime_installation_id(runtime_home)
        ))
        .filter(|value| !value.is_empty())
        .map(InstallationId::from)
}

fn runtime_name_collision_key(project_id: &str) -> String {
    project_id.to_lowercase()
}

fn legacy_runtime_matches_installation(
    runtime: &ProjectCodexRuntime,
    installation_id: &InstallationId,
) -> bool {
    let Some(path) = installation_id.as_str().strip_prefix("codex:") else {
        return false;
    };
    let legacy_home = Path::new(path);
    legacy_home.parent() == runtime.runtime_home.parent()
        && fs::symlink_metadata(legacy_home)
            .ok()
            .is_some_and(|metadata| metadata.file_type().is_symlink())
        && fs::canonicalize(legacy_home).ok() == fs::canonicalize(&runtime.runtime_home).ok()
}

fn cleanup_migration_link(path: &Path) {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => tracing::warn!(
            path = %path.display(),
            %error,
            "failed to clean up Project Codex migration link"
        ),
        _ => {}
    }
}

pub(super) fn canonical_user_home() -> Result<PathBuf, ProjectCodexRuntimeError> {
    let home = home().map_err(|error| ProjectCodexRuntimeError::Path(error.to_string()))?;
    fs::canonicalize(&home).map_err(|source| ProjectCodexRuntimeError::Io { path: home, source })
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectCodexRuntimeError {
    #[error("invalid Codex base installation: {0}")]
    InvalidBaseInstallation(String),
    #[error("invalid project directory: {0}")]
    InvalidProject(String),
    #[error("Project Codex runtime id does not match its project")]
    InvalidProjectId,
    #[error("Project Codex runtime home is outside the managed runtime root")]
    InvalidRuntimeHome,
    #[error("Project Codex runtime installation id does not match its home")]
    InvalidRuntimeInstallationId,
    #[error("Project Codex runtime state path is invalid: {path}", path = .0.display())]
    InvalidStatePath(PathBuf),
    #[error("Project Codex runtime name is already used by another project: {0}")]
    RuntimeNameConflict(String),
    #[error("Project Codex runtime home does not exist: {path}", path = .0.display())]
    MissingRuntimeHome(PathBuf),
    #[error("invalid shared auth source: {0}")]
    InvalidAuthSource(String),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to resolve Project Codex runtime path: {0}")]
    Path(String),
    #[error("failed to serialize Project Codex runtime: {0}")]
    Serialization(String),
    #[error("invalid Project Codex runtime manifest: {0}")]
    InvalidManifest(String),
    #[error("Project Codex runtime manifest is too large: {size} bytes (maximum {maximum})")]
    ManifestTooLarge { size: usize, maximum: usize },
    #[error("unsupported Project Codex runtime manifest version: {0}")]
    UnsupportedManifestVersion(u32),
}

fn default_true() -> bool {
    true
}

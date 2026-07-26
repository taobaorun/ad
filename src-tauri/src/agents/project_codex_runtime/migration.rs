use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::project_codex_runtimes_dir;

use super::super::{DiscoveryEvidence, InstallationCandidate, InstallationId};
use super::{
    canonical_runtime_installation_id, canonical_user_home, cleanup_migration_link,
    legacy_runtime_matches_installation, project_runtime_id, runtime_name_collision_key,
    scoped_base_installation_id, validate_runtime, ProjectCodexRuntime,
    ProjectCodexRuntimeDescriptor, ProjectCodexRuntimeError,
};

struct RuntimeStateEntry {
    path: PathBuf,
    runtime: ProjectCodexRuntime,
    collision_key: String,
}

pub fn persist_project_codex_runtime(
    runtime: &ProjectCodexRuntime,
) -> Result<(), ProjectCodexRuntimeError> {
    if !runtime.runtime_home.is_dir() {
        return Err(ProjectCodexRuntimeError::MissingRuntimeHome(
            runtime.runtime_home.clone(),
        ));
    }
    ensure_runtime_name_available(Path::new(&runtime.project_path), &runtime.project_id)?;
    let mut persisted = runtime.clone();
    persisted.runtime_installation_id = canonical_runtime_installation_id(&runtime.runtime_home);
    validate_runtime(&persisted)?;
    write_runtime_state(&persisted.state_path()?, &persisted)
}

pub(crate) fn discover_project_codex_candidates() -> Vec<InstallationCandidate> {
    load_project_codex_runtimes()
        .into_iter()
        .filter_map(|runtime| {
            let candidate = InstallationCandidate::from_existing_home(
                "codex",
                &runtime.runtime_home,
                DiscoveryEvidence::UserConfirmed,
            )?
            .with_project_path(runtime.project_path.clone())
            .with_base_installation_id(runtime.base_installation_id.clone());
            if candidate.installation().id != runtime.runtime_installation_id {
                tracing::warn!(
                    project_id = runtime.project_id,
                    "skipping Project Codex runtime with mismatched installation identity"
                );
                return None;
            }
            Some(candidate)
        })
        .collect()
}

pub(crate) fn runtime_for_installation(
    installation_id: &InstallationId,
) -> Option<ProjectCodexRuntime> {
    load_project_codex_runtimes()
        .into_iter()
        .find_map(|mut runtime| {
            if &runtime.runtime_installation_id == installation_id
                || legacy_runtime_matches_installation(&runtime, installation_id)
            {
                return Some(runtime);
            }
            runtime.base_installation_id =
                scoped_base_installation_id(&runtime.runtime_home, installation_id)?;
            runtime.runtime_installation_id = installation_id.clone();
            runtime.base_config_digest = None;
            runtime.generated_config_digest = None;
            Some(runtime)
        })
}

pub(crate) fn project_runtime_descriptor_for_context(
    installation_id: &InstallationId,
    project_path: &Path,
) -> Result<Option<ProjectCodexRuntimeDescriptor>, ProjectCodexRuntimeError> {
    if let Some(runtime) = runtime_for_installation(installation_id) {
        let canonical_project =
            fs::canonicalize(project_path).map_err(|source| ProjectCodexRuntimeError::Io {
                path: project_path.to_path_buf(),
                source,
            })?;
        return Ok((Path::new(&runtime.project_path) == canonical_project).then_some(runtime));
    }
    for candidate in base_codex_candidates() {
        let descriptor = ProjectCodexRuntime::derive(candidate.installation(), project_path)?;
        if &descriptor.runtime_installation_id == installation_id {
            return Ok(Some(descriptor));
        }
    }
    Ok(None)
}

pub fn project_runtime_for_base_project(
    base_installation_id: &InstallationId,
    project_path: &Path,
) -> Option<ProjectCodexRuntime> {
    let canonical_project = fs::canonicalize(project_path).ok()?;
    let mut matching = load_project_codex_runtimes()
        .into_iter()
        .filter(|runtime| Path::new(&runtime.project_path) == canonical_project)
        .collect::<Vec<_>>();
    matching
        .iter()
        .position(|runtime| &runtime.base_installation_id == base_installation_id)
        .map(|index| matching.swap_remove(index))
}

pub fn project_runtime_descriptor_for_base_project(
    base_installation_id: &InstallationId,
    project_path: &Path,
) -> Result<Option<ProjectCodexRuntimeDescriptor>, ProjectCodexRuntimeError> {
    let base = base_codex_candidates()
        .into_iter()
        .find(|candidate| &candidate.installation().id == base_installation_id);
    let Some(base) = base else {
        return Ok(None);
    };
    if let Some(runtime) = project_runtime_for_base_project(base_installation_id, project_path) {
        return Ok(Some(runtime));
    }
    ProjectCodexRuntime::derive(base.installation(), project_path).map(Some)
}

fn base_codex_candidates() -> Vec<InstallationCandidate> {
    super::super::codex::discover_codex_candidates()
        .into_iter()
        .filter(|candidate| candidate.installation().base_installation_id.is_none())
        .collect()
}

fn write_runtime_state(
    path: &Path,
    runtime: &ProjectCodexRuntime,
) -> Result<(), ProjectCodexRuntimeError> {
    let mut bytes = serde_json::to_vec_pretty(runtime)
        .map_err(|error| ProjectCodexRuntimeError::Serialization(error.to_string()))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes).map_err(|error| ProjectCodexRuntimeError::Path(error.to_string()))
}

pub(super) fn ensure_runtime_name_available(
    project_path: &Path,
    project_id: &str,
) -> Result<Option<ProjectCodexRuntime>, ProjectCodexRuntimeError> {
    let state_path = project_codex_runtimes_dir()
        .map_err(|error| ProjectCodexRuntimeError::Path(error.to_string()))?
        .join(format!("{project_id}.json"));
    if state_path.exists() {
        let state_owner = fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProjectCodexRuntime>(&bytes).ok());
        if state_owner
            .as_ref()
            .map(|runtime| Path::new(&runtime.project_path))
            != Some(project_path)
        {
            return Err(ProjectCodexRuntimeError::RuntimeNameConflict(
                project_id.into(),
            ));
        }
    }
    let collision_key = runtime_name_collision_key(project_id);
    let mut owners = read_runtime_state_entries()
        .into_iter()
        .filter(|entry| entry.collision_key == collision_key)
        .map(|entry| entry.runtime)
        .collect::<Vec<_>>();
    if owners
        .iter()
        .any(|runtime| Path::new(&runtime.project_path) != project_path)
        || owners.len() > 1
    {
        return Err(ProjectCodexRuntimeError::RuntimeNameConflict(
            project_id.to_owned(),
        ));
    }
    Ok(owners.pop())
}

pub(super) fn load_project_codex_runtimes() -> Vec<ProjectCodexRuntime> {
    let entries = read_runtime_state_entries();
    let mut group_sizes = BTreeMap::<String, usize>::new();
    for entry in &entries {
        *group_sizes.entry(entry.collision_key.clone()).or_default() += 1;
    }

    let mut runtimes = entries
        .into_iter()
        .filter_map(|entry| {
            let grouped_conflict = group_sizes
                .get(&entry.collision_key)
                .is_some_and(|count| *count > 1);
            let result = if grouped_conflict {
                tracing::warn!(
                    project_path = entry.runtime.project_path,
                    "keeping conflicting legacy Project Codex runtime without migration"
                );
                validate_unmigrated_runtime(&entry.path, &entry.runtime).map(|_| entry.runtime)
            } else {
                migrate_legacy_runtime(&entry.path, entry.runtime)
            };
            match result {
                Ok(runtime) => Some(runtime),
                Err(error) => {
                    tracing::warn!(
                        path = %entry.path.display(),
                        %error,
                        "skipping invalid Project Codex runtime"
                    );
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    runtimes.sort_by(|left, right| {
        left.runtime_installation_id
            .cmp(&right.runtime_installation_id)
    });
    runtimes.dedup_by(|left, right| left.runtime_installation_id == right.runtime_installation_id);
    runtimes
}

fn read_runtime_state_entries() -> Vec<RuntimeStateEntry> {
    let Ok(directory) = project_codex_runtimes_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                return None;
            }
            let runtime = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<ProjectCodexRuntime>(&bytes).ok());
            let Some(runtime) = runtime else {
                tracing::warn!(path = %path.display(), "skipping malformed Project Codex runtime");
                return None;
            };
            let project_id = project_runtime_id(Path::new(&runtime.project_path)).ok()?;
            Some(RuntimeStateEntry {
                path,
                runtime,
                collision_key: runtime_name_collision_key(&project_id),
            })
        })
        .collect()
}

fn validate_unmigrated_runtime(
    state_path: &Path,
    runtime: &ProjectCodexRuntime,
) -> Result<(), ProjectCodexRuntimeError> {
    validate_project_path(runtime)?;
    let expected_project_id = project_runtime_id(Path::new(&runtime.project_path))?;
    if runtime.project_id == expected_project_id {
        if state_path.file_stem().and_then(|stem| stem.to_str())
            != Some(expected_project_id.as_str())
        {
            return Err(ProjectCodexRuntimeError::InvalidStatePath(
                state_path.to_path_buf(),
            ));
        }
        return validate_runtime(runtime);
    }
    validate_legacy_runtime(state_path, runtime)
}

fn validate_legacy_runtime(
    state_path: &Path,
    runtime: &ProjectCodexRuntime,
) -> Result<(), ProjectCodexRuntimeError> {
    validate_project_path(runtime)?;
    let legacy_id = legacy_project_id(&runtime.project_path, &runtime.base_installation_id);
    if runtime.project_id != legacy_id
        || state_path.file_stem().and_then(|stem| stem.to_str()) != Some(legacy_id.as_str())
    {
        return Err(ProjectCodexRuntimeError::InvalidProjectId);
    }
    let legacy_home = canonical_user_home()?
        .join(".ad/codex-homes")
        .join(&legacy_id);
    if runtime.runtime_home != legacy_home
        || runtime.runtime_installation_id != canonical_runtime_installation_id(&legacy_home)
    {
        return Err(ProjectCodexRuntimeError::InvalidRuntimeHome);
    }
    Ok(())
}

fn validate_project_path(runtime: &ProjectCodexRuntime) -> Result<(), ProjectCodexRuntimeError> {
    let canonical =
        fs::canonicalize(&runtime.project_path).map_err(|source| ProjectCodexRuntimeError::Io {
            path: PathBuf::from(&runtime.project_path),
            source,
        })?;
    if !canonical.is_dir() || canonical.to_string_lossy() != runtime.project_path.as_str() {
        return Err(ProjectCodexRuntimeError::InvalidProject(
            runtime.project_path.clone(),
        ));
    }
    Ok(())
}

fn legacy_project_id(project_path: &str, base_installation_id: &InstallationId) -> String {
    let mut digest = Sha256::new();
    digest.update(project_path.as_bytes());
    digest.update([0]);
    digest.update(base_installation_id.as_str().as_bytes());
    let hex = format!("{:x}", digest.finalize());
    hex[..24].to_owned()
}

fn migrate_legacy_runtime(
    state_path: &Path,
    mut runtime: ProjectCodexRuntime,
) -> Result<ProjectCodexRuntime, ProjectCodexRuntimeError> {
    let expected_project_id = project_runtime_id(Path::new(&runtime.project_path))?;
    let state_directory = state_path
        .parent()
        .ok_or_else(|| ProjectCodexRuntimeError::InvalidStatePath(state_path.to_path_buf()))?;
    let expected_state_path = state_directory.join(format!("{expected_project_id}.json"));

    if runtime.project_id == expected_project_id {
        validate_runtime(&runtime)?;
        if state_path != expected_state_path {
            let legacy_id = legacy_project_id(&runtime.project_path, &runtime.base_installation_id);
            if state_path != state_directory.join(format!("{legacy_id}.json"))
                || expected_state_path.exists()
            {
                return Err(ProjectCodexRuntimeError::InvalidStatePath(
                    state_path.to_path_buf(),
                ));
            }
            fs::rename(state_path, &expected_state_path).map_err(|source| {
                ProjectCodexRuntimeError::Io {
                    path: state_path.to_path_buf(),
                    source,
                }
            })?;
        }
        return Ok(runtime);
    }

    validate_legacy_runtime(state_path, &runtime)?;
    if expected_state_path.exists() {
        return Err(ProjectCodexRuntimeError::RuntimeNameConflict(
            expected_project_id,
        ));
    }
    let legacy_home = runtime.runtime_home.clone();
    let named_home = canonical_user_home()?
        .join(".ad/codex-homes")
        .join(&expected_project_id);
    migrate_legacy_runtime_home_with(&legacy_home, &named_home, |from, to| fs::rename(from, to))?;
    runtime.project_id = expected_project_id;
    runtime.runtime_home = named_home;
    runtime.runtime_installation_id = canonical_runtime_installation_id(&runtime.runtime_home);
    write_runtime_state(state_path, &runtime)?;
    fs::rename(state_path, &expected_state_path).map_err(|source| {
        ProjectCodexRuntimeError::Io {
            path: state_path.to_path_buf(),
            source,
        }
    })?;
    Ok(runtime)
}

fn migrate_legacy_runtime_home_with(
    legacy_home: &Path,
    named_home: &Path,
    finalize_link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), ProjectCodexRuntimeError> {
    match fs::symlink_metadata(legacy_home) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let legacy_target =
                fs::canonicalize(legacy_home).map_err(|source| ProjectCodexRuntimeError::Io {
                    path: legacy_home.to_path_buf(),
                    source,
                })?;
            let named_target =
                fs::canonicalize(named_home).map_err(|source| ProjectCodexRuntimeError::Io {
                    path: named_home.to_path_buf(),
                    source,
                })?;
            if legacy_target != named_target || !named_target.is_dir() {
                return Err(ProjectCodexRuntimeError::RuntimeNameConflict(
                    named_home.to_string_lossy().into_owned(),
                ));
            }
            return Ok(());
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ProjectCodexRuntimeError::InvalidRuntimeHome),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && named_home.is_dir() => {
            return std::os::unix::fs::symlink(named_home, legacy_home).map_err(|source| {
                ProjectCodexRuntimeError::Io {
                    path: legacy_home.to_path_buf(),
                    source,
                }
            });
        }
        Err(source) => {
            return Err(ProjectCodexRuntimeError::Io {
                path: legacy_home.to_path_buf(),
                source,
            });
        }
    }
    if named_home.exists() {
        return Err(ProjectCodexRuntimeError::RuntimeNameConflict(
            named_home.to_string_lossy().into_owned(),
        ));
    }

    let temporary_link = legacy_home.with_file_name(format!(
        ".{}.migration-{}.tmp",
        legacy_home
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project-codex"),
        Uuid::new_v4()
    ));
    std::os::unix::fs::symlink(named_home, &temporary_link).map_err(|source| {
        ProjectCodexRuntimeError::Io {
            path: temporary_link.clone(),
            source,
        }
    })?;
    if let Err(source) = fs::rename(legacy_home, named_home) {
        cleanup_migration_link(&temporary_link);
        return Err(ProjectCodexRuntimeError::Io {
            path: legacy_home.to_path_buf(),
            source,
        });
    }
    if let Err(source) = finalize_link(&temporary_link, legacy_home) {
        if let Err(rollback_error) = fs::rename(named_home, legacy_home) {
            tracing::error!(
                path = %named_home.display(),
                error = %rollback_error,
                "failed to roll back Project Codex runtime home migration"
            );
        }
        cleanup_migration_link(&temporary_link);
        return Err(ProjectCodexRuntimeError::Io {
            path: legacy_home.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_compatibility_link_rolls_back_the_legacy_home() {
        let temp = tempfile::tempdir().unwrap();
        let legacy_home = temp.path().join("legacy");
        let named_home = temp.path().join("project");
        fs::create_dir(&legacy_home).unwrap();

        let result = migrate_legacy_runtime_home_with(&legacy_home, &named_home, |_, _| {
            Err(std::io::Error::other("injected link failure"))
        });

        assert!(result.is_err());
        assert!(legacy_home.is_dir());
        assert!(!named_home.exists());
    }
}

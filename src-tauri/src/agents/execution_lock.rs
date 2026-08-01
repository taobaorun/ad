use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rustix::fs::{flock, FlockOperation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fs::paths::execution_locks_dir;

use super::execution_state::{ExecutionState, StateDirectory};
use super::{AdapterRegistry, AgentContext, AgentError, AgentErrorCode, MutationPlan, ResourceRef};

const LOCK_SCHEMA_VERSION: u32 = 1;
static INSTANCE_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LockMetadata {
    schema_version: u32,
    instance_id: String,
    operation_id: String,
}

/// A stable ordered set of advisory locks held until this value is dropped.
#[derive(Debug)]
pub struct TargetLockSet {
    files: Vec<File>,
}

impl TargetLockSet {
    pub(super) fn acquire_for_plan(
        plan: &MutationPlan,
        registry: &AdapterRegistry,
        state: &ExecutionState,
    ) -> Result<Self, AgentError> {
        let resources = plan
            .read_set
            .iter()
            .map(|precondition| precondition.resource.clone())
            .chain(
                plan.mutations
                    .iter()
                    .map(|mutation| mutation.resource.clone()),
            )
            .collect::<Vec<_>>();
        Self::acquire_for_resources(&resources, plan.id.as_str(), registry, state)
    }

    pub(super) fn acquire_for_resources(
        resources: &[ResourceRef],
        operation_id: &str,
        registry: &AdapterRegistry,
        state: &ExecutionState,
    ) -> Result<Self, AgentError> {
        let mut targets = Vec::with_capacity(resources.len());
        for resource in resources {
            let context = AgentContext {
                installation_id: resource.installation_id.clone(),
                project_path: resource.project_path.clone(),
            };
            targets.push(
                registry
                    .resolve_resource(&context, resource)?
                    .path()
                    .to_path_buf(),
            );
        }
        Self::acquire_in(
            state.locks(),
            &targets,
            execution_instance_id(),
            operation_id,
        )
        .map_err(|error| AgentError {
            code: if error.kind() == std::io::ErrorKind::WouldBlock {
                AgentErrorCode::ResourceChanged
            } else {
                AgentErrorCode::Io
            },
            message: format!("Failed to acquire execution target locks: {error}"),
            agent_id: None,
            installation_id: resources
                .first()
                .map(|resource| resource.installation_id.clone()),
            resource: resources.first().cloned(),
            retryable: error.kind() == std::io::ErrorKind::WouldBlock,
            details: None,
        })
    }

    pub fn acquire(
        targets: &[PathBuf],
        instance_id: &str,
        operation_id: &str,
    ) -> Result<Self, std::io::Error> {
        let root =
            execution_locks_dir().map_err(|error| std::io::Error::other(error.to_string()))?;
        Self::acquire_at(&root, targets, instance_id, operation_id)
    }

    pub fn acquire_at(
        root: &Path,
        targets: &[PathBuf],
        instance_id: &str,
        operation_id: &str,
    ) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(root)?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        sync_directory(root)?;
        let mut canonical_targets = targets
            .iter()
            .map(|target| canonical_physical_target(target))
            .collect::<Result<Vec<_>, _>>()?;
        canonical_targets.sort_by(|left, right| {
            left.as_os_str()
                .as_bytes()
                .cmp(right.as_os_str().as_bytes())
        });
        canonical_targets.dedup();

        let mut files = Vec::with_capacity(canonical_targets.len());
        for target in canonical_targets {
            let path = root.join(format!("{}.lock", target_fingerprint(&target)));
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            if let Err(error) = flock(&file, FlockOperation::NonBlockingLockExclusive) {
                return Err(if error == rustix::io::Errno::WOULDBLOCK {
                    std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        format!("Mutation target is locked: {}", target.display()),
                    )
                } else {
                    error.into()
                });
            }
            validate_previous_metadata(&mut file, &path)?;
            write_metadata(
                &mut file,
                &LockMetadata {
                    schema_version: LOCK_SCHEMA_VERSION,
                    instance_id: instance_id.to_owned(),
                    operation_id: operation_id.to_owned(),
                },
            )?;
            sync_directory(root)?;
            files.push(file);
        }
        Ok(Self { files })
    }

    fn acquire_in(
        root: &StateDirectory,
        targets: &[PathBuf],
        instance_id: &str,
        operation_id: &str,
    ) -> Result<Self, std::io::Error> {
        let mut canonical_targets = targets
            .iter()
            .map(|target| canonical_physical_target(target))
            .collect::<Result<Vec<_>, _>>()?;
        canonical_targets.sort_by(|left, right| {
            left.as_os_str()
                .as_bytes()
                .cmp(right.as_os_str().as_bytes())
        });
        canonical_targets.dedup();

        let mut files = Vec::with_capacity(canonical_targets.len());
        for target in canonical_targets {
            let name = format!("{}.lock", target_fingerprint(&target));
            let path = root.display_path().join(&name);
            let mut file = root.open_lock(&name)?;
            if let Err(error) = flock(&file, FlockOperation::NonBlockingLockExclusive) {
                return Err(if error == rustix::io::Errno::WOULDBLOCK {
                    std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        format!("Mutation target is locked: {}", target.display()),
                    )
                } else {
                    error.into()
                });
            }
            validate_previous_metadata(&mut file, &path)?;
            write_metadata(
                &mut file,
                &LockMetadata {
                    schema_version: LOCK_SCHEMA_VERSION,
                    instance_id: instance_id.to_owned(),
                    operation_id: operation_id.to_owned(),
                },
            )?;
            files.push(file);
        }
        Ok(Self { files })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Drop for TargetLockSet {
    fn drop(&mut self) {
        for file in self.files.iter().rev() {
            if let Err(error) = flock(file, FlockOperation::Unlock) {
                tracing::warn!(%error, "Failed to release an execution target lock");
            }
        }
    }
}

pub fn execution_instance_id() -> &'static str {
    INSTANCE_ID
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .as_str()
}

fn canonical_physical_target(target: &Path) -> Result<PathBuf, std::io::Error> {
    if !target.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Mutation target is not absolute: {}", target.display()),
        ));
    }
    let mut existing = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Mutation target has no parent: {}", target.display()),
        )
    })?;
    let mut suffix = vec![target.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Mutation target has no name: {}", target.display()),
        )
    })?];
    while !existing.exists() {
        suffix.push(existing.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Mutation target has no existing ancestor: {}",
                    target.display()
                ),
            )
        })?);
        existing = existing.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Mutation target has no existing ancestor: {}",
                    target.display()
                ),
            )
        })?;
    }
    let mut canonical = std::fs::canonicalize(existing)?;
    for component in suffix.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn target_fingerprint(target: &Path) -> String {
    format!("{:x}", Sha256::digest(target.as_os_str().as_bytes()))
}

fn validate_previous_metadata(file: &mut File, path: &Path) -> Result<(), std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Ok(());
    }
    let metadata: LockMetadata = serde_json::from_slice(&bytes).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Invalid execution lock metadata at {}: {error}",
                path.display()
            ),
        )
    })?;
    if metadata.schema_version != LOCK_SCHEMA_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Unsupported execution lock schema {} at {}",
                metadata.schema_version,
                path.display()
            ),
        ));
    }
    Ok(())
}

fn write_metadata(file: &mut File, metadata: &LockMetadata) -> Result<(), std::io::Error> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_aliases_acquire_one_lock() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("project");
        std::fs::create_dir_all(&parent).unwrap();
        let target = parent.join("config.toml");

        let locks = TargetLockSet::acquire_at(
            &temp.path().join("locks"),
            &[target.clone(), parent.join("./config.toml"), target],
            "instance",
            "operation",
        )
        .unwrap();

        assert_eq!(locks.len(), 1);
    }

    #[test]
    fn incompatible_persisted_lock_metadata_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("project");
        let lock_root = temp.path().join("locks");
        std::fs::create_dir_all(&parent).unwrap();
        let target = parent.join("config.toml");
        let locks = TargetLockSet::acquire_at(
            &lock_root,
            std::slice::from_ref(&target),
            "instance",
            "operation",
        )
        .unwrap();
        drop(locks);
        let lock_path = std::fs::read_dir(&lock_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(
            &lock_path,
            r#"{"schemaVersion":99,"instanceId":"old","operationId":"old"}"#,
        )
        .unwrap();

        let error =
            TargetLockSet::acquire_at(&lock_root, &[target], "instance", "operation").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}

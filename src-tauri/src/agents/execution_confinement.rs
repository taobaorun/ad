use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fstat, fsync, mkdirat, open, openat, renameat, statat, unlinkat, AtFlags, FileType, Mode,
    OFlags,
};

use crate::fs::paths::{ad_home, claude_dir, codex_dir};

use super::execution_fs::TargetState;
use super::{AgentError, AgentErrorCode, ResourceRef};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug)]
pub(super) struct ConfinedFileTarget {
    parent: OwnedFd,
    name: OsString,
    resource: ResourceRef,
    display_path: PathBuf,
}

impl ConfinedFileTarget {
    pub(super) fn resolve(resource: &ResourceRef, target: &Path) -> Result<Self, AgentError> {
        let root = trusted_root(resource, target)?;
        let relative = target.strip_prefix(&root).map_err(|_| {
            confinement_error(
                resource,
                format!(
                    "Mutation target {} escapes trusted root {}",
                    target.display(),
                    root.display()
                ),
            )
        })?;
        let name = relative
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| confinement_error(resource, "Mutation target has no file name"))?
            .to_os_string();
        let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
        let root_fd = open_trusted_directory(&root, resource)?;
        let parent = open_relative_directories(root_fd, parent_relative, resource)?;
        Ok(Self {
            parent,
            name,
            resource: resource.clone(),
            display_path: target.to_path_buf(),
        })
    }

    pub(super) fn observe(&self) -> Result<TargetState, AgentError> {
        let stat = match statat(
            &self.parent,
            self.name.as_os_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(TargetState::Missing),
            Err(error) => return Err(self.io_error(error.into())),
        };
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
            return Err(confinement_error(
                &self.resource,
                format!(
                    "Managed file target is not a regular file: {}",
                    self.display_path.display()
                ),
            ));
        }
        let fd = openat(
            &self.parent,
            self.name.as_os_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| self.io_error(error.into()))?;
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| self.io_error(error))?;
        Ok(TargetState::File(bytes))
    }

    pub(super) fn write_atomic(&self, bytes: &[u8]) -> Result<(), AgentError> {
        let temporary = temporary_name(&self.name);
        let result = (|| -> Result<(), std::io::Error> {
            let fd = openat(
                &self.parent,
                temporary.as_os_str(),
                OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(std::io::Error::from)?;
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            file.sync_all()?;
            renameat(
                &self.parent,
                temporary.as_os_str(),
                &self.parent,
                self.name.as_os_str(),
            )
            .map_err(std::io::Error::from)?;
            fsync(&self.parent).map_err(std::io::Error::from)
        })();
        if let Err(error) = result {
            let _ = unlinkat(&self.parent, temporary.as_os_str(), AtFlags::empty());
            return Err(self.io_error(error));
        }
        Ok(())
    }

    pub(super) fn remove(&self) -> Result<(), AgentError> {
        match unlinkat(&self.parent, self.name.as_os_str(), AtFlags::empty()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => {
                fsync(&self.parent).map_err(|error| self.io_error(error.into()))
            }
            Err(error) => Err(self.io_error(error.into())),
        }
    }

    fn io_error(&self, error: std::io::Error) -> AgentError {
        AgentError {
            code: if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
            ) {
                AgentErrorCode::PermissionDenied
            } else {
                AgentErrorCode::Io
            },
            message: format!(
                "Failed to access confined target {}: {error}",
                self.display_path.display()
            ),
            agent_id: None,
            installation_id: Some(self.resource.installation_id.clone()),
            resource: Some(self.resource.clone()),
            retryable: false,
            details: None,
        }
    }
}

pub(super) fn validate_ad_managed_root() -> Result<(), AgentError> {
    let root = ad_home().map_err(|error| ad_root_error(error.to_string()))?;
    if !root.exists() {
        std::fs::create_dir(&root).map_err(|error| ad_root_error(error.to_string()))?;
        std::fs::set_permissions(
            &root,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .map_err(|error| ad_root_error(error.to_string()))?;
    }
    let fd = open(&root, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|error| ad_root_error(error.to_string()))?;
    validate_directory_fd(&fd, &root).map_err(ad_root_error)?;
    let state = ensure_ad_directory(&fd, "state", &root.join("state"))?;
    ensure_ad_directory(
        &state,
        "execution-locks",
        &root.join("state/execution-locks"),
    )?;
    ensure_ad_directory(
        &state,
        "operation-journals",
        &root.join("state/operation-journals"),
    )?;
    let backups = ensure_ad_directory(&fd, "backups", &root.join("backups"))?;
    ensure_ad_directory(&backups, "operations", &root.join("backups/operations"))?;
    let history = ensure_ad_directory(&fd, "history", &root.join("history"))?;
    ensure_ad_directory(&history, "operations", &root.join("history/operations"))?;
    Ok(())
}

fn ensure_ad_directory(
    parent: &OwnedFd,
    name: &str,
    display_path: &Path,
) -> Result<OwnedFd, AgentError> {
    let fd = match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            mkdirat(parent, name, Mode::RWXU).map_err(|error| ad_root_error(error.to_string()))?;
            openat(parent, name, DIRECTORY_FLAGS, Mode::empty())
                .map_err(|error| ad_root_error(error.to_string()))?
        }
        Err(error) => return Err(ad_root_error(error.to_string())),
    };
    validate_directory_fd(&fd, display_path).map_err(ad_root_error)?;
    Ok(fd)
}

fn trusted_root(resource: &ResourceRef, target: &Path) -> Result<PathBuf, AgentError> {
    let ad_root = ad_home().map_err(|error| confinement_error(resource, error.to_string()))?;
    validate_ad_managed_root()?;
    let canonical_ad_root = std::fs::canonicalize(&ad_root)
        .map_err(|error| confinement_error(resource, error.to_string()))?;
    if target.starts_with(&canonical_ad_root) {
        return Ok(canonical_ad_root);
    }
    if let Some(project_path) = resource.project_path.as_deref() {
        let project = PathBuf::from(project_path);
        if target.starts_with(&project) {
            return Ok(project);
        }
    }
    trusted_user_agent_root(resource, target)
}

fn trusted_user_agent_root(resource: &ResourceRef, target: &Path) -> Result<PathBuf, AgentError> {
    let (agent_id, expected_root) = resource
        .installation_id
        .as_str()
        .split_once(':')
        .ok_or_else(|| confinement_error(resource, "Invalid Agent installation identity"))?;
    let expected_root = Path::new(expected_root);
    let mut candidates = Vec::new();
    match agent_id {
        "claude-code" => candidates
            .push(claude_dir().map_err(|error| confinement_error(resource, error.to_string()))?),
        "codex" => {
            if let Ok(environment_home) = std::env::var("CODEX_HOME") {
                candidates.push(PathBuf::from(environment_home));
            }
            candidates
                .push(codex_dir().map_err(|error| confinement_error(resource, error.to_string()))?);
        }
        _ => {
            return Err(confinement_error(
                resource,
                format!("Unsupported Agent target root: {agent_id}"),
            ))
        }
    }
    for candidate in candidates {
        let Ok(canonical) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if canonical == expected_root {
            open_trusted_directory(&candidate, resource)?;
            if !target.starts_with(expected_root) {
                return Err(confinement_error(
                    resource,
                    "Agent target escapes its installation root",
                ));
            }
            return Ok(expected_root.to_path_buf());
        }
    }
    Err(confinement_error(
        resource,
        "Agent installation is not backed by a trusted configured root",
    ))
}

fn open_trusted_directory(root: &Path, resource: &ResourceRef) -> Result<OwnedFd, AgentError> {
    let fd = open(root, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|error| confinement_error(resource, error.to_string()))?;
    validate_directory_fd(&fd, root).map_err(|error| confinement_error(resource, error))?;
    Ok(fd)
}

fn validate_directory_fd(fd: &OwnedFd, path: &Path) -> Result<(), String> {
    let stat = fstat(fd).map_err(|error| error.to_string())?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(format!(
            "Trusted root is not a directory: {}",
            path.display()
        ));
    }
    let expected_uid = rustix::process::geteuid().as_raw();
    if stat.st_uid != expected_uid {
        return Err(format!(
            "Trusted root is not owned by the current user: {}",
            path.display()
        ));
    }
    let mode = stat.st_mode & 0o777;
    let unsafe_mode = mode & 0o022 != 0;
    if unsafe_mode {
        return Err(format!(
            "Trusted root permissions are too broad ({mode:o}): {}",
            path.display()
        ));
    }
    Ok(())
}

fn open_relative_directories(
    mut current: OwnedFd,
    relative: &Path,
    resource: &ResourceRef,
) -> Result<OwnedFd, AgentError> {
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(confinement_error(
                resource,
                "Mutation target contains an unsafe path component",
            ));
        };
        let next = match openat(&current, name, DIRECTORY_FLAGS, Mode::empty()) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT) => {
                mkdirat(&current, name, Mode::RWXU)
                    .map_err(|error| confinement_error(resource, error.to_string()))?;
                openat(&current, name, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(|error| confinement_error(resource, error.to_string()))?
            }
            Err(error) => return Err(confinement_error(resource, error.to_string())),
        };
        current = next;
    }
    Ok(current)
}

fn temporary_name(name: &OsStr) -> OsString {
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".tmp.{}", uuid::Uuid::new_v4().simple()));
    temporary
}

fn confinement_error(resource: &ResourceRef, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: message.into(),
        agent_id: None,
        installation_id: Some(resource.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: false,
        details: None,
    }
}

fn ad_root_error(message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: format!("Unsafe AD data root: {}", message.into()),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{InstallationId, ResourceKind, ResourceScope};

    #[test]
    fn held_parent_fd_prevents_ancestor_swap_escape() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let original_parent = project.join(".claude");
        let moved_parent = project.join(".claude.original");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&original_parent).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(original_parent.join("settings.json"), b"original").unwrap();
        std::fs::write(outside.join("settings.json"), b"outside").unwrap();
        let project = std::fs::canonicalize(project).unwrap();
        let resource = ResourceRef {
            installation_id: InstallationId::from("claude-code:test"),
            project_path: Some(project.to_string_lossy().into_owned()),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "project-shared".into(),
        };
        let confined =
            ConfinedFileTarget::resolve(&resource, &project.join(".claude/settings.json")).unwrap();

        std::fs::rename(&original_parent, &moved_parent).unwrap();
        std::os::unix::fs::symlink(&outside, &original_parent).unwrap();
        confined.write_atomic(b"confined").unwrap();

        assert_eq!(
            std::fs::read(moved_parent.join("settings.json")).unwrap(),
            b"confined"
        );
        assert_eq!(
            std::fs::read(outside.join("settings.json")).unwrap(),
            b"outside"
        );
    }
}

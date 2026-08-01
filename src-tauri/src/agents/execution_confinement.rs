use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fstat, fsync, mkdirat, open, openat, readlinkat, renameat, statat, unlinkat, AtFlags, FileType,
    Mode, OFlags,
};

use crate::fs::paths::{ad_home, claude_dir, codex_dir};

use super::execution_fs::TargetState;
use super::{AgentError, AgentErrorCode, ResourceRef};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug)]
pub(super) struct ConfinedTarget {
    parent: OwnedFd,
    pending_parents: Vec<OsString>,
    name: OsString,
    resource: ResourceRef,
    display_path: PathBuf,
}

impl ConfinedTarget {
    pub(super) fn observe_dependency(
        resource: &ResourceRef,
        target: &Path,
    ) -> Result<TargetState, AgentError> {
        Self::resolve_internal(resource, target, true)?.observe()
    }

    pub(super) fn observe_existing(
        resource: &ResourceRef,
        target: &Path,
    ) -> Result<TargetState, AgentError> {
        Self::resolve_internal(resource, target, false)?.observe()
    }

    pub(super) fn resolve(resource: &ResourceRef, target: &Path) -> Result<Self, AgentError> {
        Self::resolve_internal(resource, target, false)
    }

    fn resolve_internal(
        resource: &ResourceRef,
        target: &Path,
        allow_dependency_root: bool,
    ) -> Result<Self, AgentError> {
        let root = if allow_dependency_root {
            trusted_observation_root(resource, target)?
        } else {
            trusted_root(resource, target)?
        };
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
        let (parent, pending_parents) = resolve_parent(root_fd, parent_relative, resource)?;
        Ok(Self {
            parent,
            pending_parents,
            name,
            resource: resource.clone(),
            display_path: target.to_path_buf(),
        })
    }

    pub(super) fn observe(&self) -> Result<TargetState, AgentError> {
        let Some(parent) = self.parent(false)? else {
            return Ok(TargetState::Missing);
        };
        let stat = match statat(&parent, self.name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(TargetState::Missing),
            Err(error) => return Err(self.io_error(error.into())),
        };
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => {
                let fd = openat(
                    &parent,
                    self.name.as_os_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW,
                    Mode::empty(),
                )
                .map_err(|error| self.io_error(error.into()))?;
                let mut bytes = Vec::new();
                File::from(fd)
                    .read_to_end(&mut bytes)
                    .map_err(|error| self.io_error(error))?;
                Ok(TargetState::File(bytes))
            }
            FileType::Symlink => readlinkat(&parent, self.name.as_os_str(), Vec::new())
                .map(|link| {
                    TargetState::Symlink(OsString::from_vec(link.as_bytes().to_vec()).into())
                })
                .map_err(|error| self.io_error(error.into())),
            FileType::Directory => {
                super::execution_tree::directory_digest(&parent, self.name.as_os_str())
                    .map(TargetState::Directory)
                    .map_err(|error| self.io_error(error))
            }
            _ => Err(confinement_error(
                &self.resource,
                format!(
                    "Unsupported managed target: {}",
                    self.display_path.display()
                ),
            )),
        }
    }

    pub(super) fn write_atomic(&self, bytes: &[u8]) -> Result<(), AgentError> {
        let parent = self
            .parent(true)?
            .expect("creating target parents always returns a directory");
        let temporary = temporary_name(&self.name);
        let result = (|| -> Result<(), std::io::Error> {
            let fd = openat(
                &parent,
                temporary.as_os_str(),
                OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(std::io::Error::from)?;
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            file.sync_all()?;
            renameat(
                &parent,
                temporary.as_os_str(),
                &parent,
                self.name.as_os_str(),
            )
            .map_err(std::io::Error::from)?;
            fsync(&parent).map_err(std::io::Error::from)
        })();
        if let Err(error) = result {
            let _ = unlinkat(&parent, temporary.as_os_str(), AtFlags::empty());
            return Err(self.io_error(error));
        }
        Ok(())
    }

    pub(super) fn remove(&self) -> Result<(), AgentError> {
        let Some(parent) = self.parent(false)? else {
            return Ok(());
        };
        super::execution_tree::remove_entry(&parent, self.name.as_os_str())
            .map_err(|error| self.io_error(error))
    }

    pub(super) fn write_symlink_atomic(&self, source: &Path) -> Result<(), AgentError> {
        let parent = self
            .parent(true)?
            .expect("creating target parents always returns a directory");
        super::execution_tree::write_symlink_atomic(&parent, self.name.as_os_str(), source)
            .map_err(|error| self.io_error(error))
    }

    pub(super) fn write_directory_atomic(&self, source: &Path) -> Result<(), AgentError> {
        let parent = self
            .parent(true)?
            .expect("creating target parents always returns a directory");
        super::execution_tree::write_directory_atomic(&parent, self.name.as_os_str(), source)
            .map_err(|error| self.io_error(error))
    }

    pub(super) fn copy_directory_to(&self, destination: &Path) -> Result<(), AgentError> {
        let parent = self.parent(false)?.ok_or_else(|| {
            self.io_error(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Directory target parent no longer exists",
            ))
        })?;
        super::execution_tree::copy_directory_to(&parent, self.name.as_os_str(), destination)
            .map_err(|error| self.io_error(error))
    }

    fn parent(&self, create: bool) -> Result<Option<OwnedFd>, AgentError> {
        let mut current =
            rustix::io::dup(&self.parent).map_err(|error| self.io_error(error.into()))?;
        for name in &self.pending_parents {
            current = match openat(&current, name, DIRECTORY_FLAGS, Mode::empty()) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::NOENT) if !create => return Ok(None),
                Err(rustix::io::Errno::NOENT) => {
                    mkdirat(&current, name, Mode::RWXU)
                        .map_err(|error| self.io_error(error.into()))?;
                    fsync(&current).map_err(|error| self.io_error(error.into()))?;
                    openat(&current, name, DIRECTORY_FLAGS, Mode::empty())
                        .map_err(|error| self.io_error(error.into()))?
                }
                Err(error) => return Err(self.io_error(error.into())),
            };
            validate_directory_fd(&current, &self.display_path)
                .map_err(|error| confinement_error(&self.resource, error))?;
        }
        Ok(Some(current))
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
    if let Some(root) = trusted_scoped_root(resource, target)? {
        return Ok(root);
    }
    trusted_user_agent_root(resource, target)
}

fn trusted_observation_root(resource: &ResourceRef, target: &Path) -> Result<PathBuf, AgentError> {
    if let Some(root) = trusted_scoped_root(resource, target)? {
        return Ok(root);
    }
    let (agent_id, _) = resource
        .installation_id
        .as_str()
        .split_once(':')
        .ok_or_else(|| confinement_error(resource, "Invalid Agent installation identity"))?;
    for candidate in agent_root_candidates(resource, agent_id)? {
        let Ok(canonical) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if target.starts_with(&canonical) {
            open_trusted_directory(&candidate, resource)?;
            return Ok(canonical);
        }
    }
    Err(confinement_error(
        resource,
        "Agent dependency is not backed by a trusted configured root",
    ))
}

fn trusted_scoped_root(
    resource: &ResourceRef,
    target: &Path,
) -> Result<Option<PathBuf>, AgentError> {
    let ad_root = ad_home().map_err(|error| confinement_error(resource, error.to_string()))?;
    let canonical_ad_root = std::fs::canonicalize(&ad_root).ok();
    if target.starts_with(&ad_root)
        || canonical_ad_root
            .as_ref()
            .is_some_and(|root| target.starts_with(root))
    {
        validate_ad_managed_root()?;
        return std::fs::canonicalize(&ad_root)
            .map(Some)
            .map_err(|error| confinement_error(resource, error.to_string()));
    }
    if let Some(project_path) = resource.project_path.as_deref() {
        let project = PathBuf::from(project_path);
        if target.starts_with(&project) {
            return Ok(Some(project));
        }
    }
    Ok(None)
}

fn trusted_user_agent_root(resource: &ResourceRef, target: &Path) -> Result<PathBuf, AgentError> {
    let (agent_id, expected_root) = resource
        .installation_id
        .as_str()
        .split_once(':')
        .ok_or_else(|| confinement_error(resource, "Invalid Agent installation identity"))?;
    let expected_root = Path::new(expected_root);
    for candidate in agent_root_candidates(resource, agent_id)? {
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

fn agent_root_candidates(
    resource: &ResourceRef,
    agent_id: &str,
) -> Result<Vec<PathBuf>, AgentError> {
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
    Ok(candidates)
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

fn resolve_parent(
    mut current: OwnedFd,
    relative: &Path,
    resource: &ResourceRef,
) -> Result<(OwnedFd, Vec<OsString>), AgentError> {
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name.to_os_string()),
            _ => Err(confinement_error(
                resource,
                "Mutation target contains an unsafe path component",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, name) in components.iter().enumerate() {
        match openat(&current, name, DIRECTORY_FLAGS, Mode::empty()) {
            Ok(fd) => {
                validate_directory_fd(&fd, relative)
                    .map_err(|error| confinement_error(resource, error))?;
                current = fd;
            }
            Err(rustix::io::Errno::NOENT) => return Ok((current, components[index..].to_vec())),
            Err(error) => return Err(confinement_error(resource, error.to_string())),
        }
    }
    Ok((current, Vec::new()))
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

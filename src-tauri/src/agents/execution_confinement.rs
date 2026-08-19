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
use super::execution_state::{ExecutionState, StateDirectory};
use super::{AgentContext, AgentError, AgentErrorCode, ResourceRef};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProjectRootIdentity {
    device: u64,
    inode: u64,
}

pub(super) fn capture_project_root_identity(
    context: &AgentContext,
) -> Result<Option<ProjectRootIdentity>, AgentError> {
    let Some(project_path) = context.project_path.as_deref() else {
        return Ok(None);
    };
    let project = Path::new(project_path);
    validate_project_workspace_root(project).map_err(|message| AgentError {
        code: AgentErrorCode::PermissionDenied,
        message,
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "execution_confinement"})),
    })?;
    let fd = open_absolute_directory_nofollow(project).map_err(|error| AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: format!(
            "Failed to bind the project workspace root {}: {error}",
            project.display()
        ),
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "execution_confinement"})),
    })?;
    validate_directory_fd(&fd, project).map_err(|message| AgentError {
        code: AgentErrorCode::PermissionDenied,
        message,
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "execution_confinement"})),
    })?;
    project_root_identity(&fd)
        .map(Some)
        .map_err(|error| AgentError {
            code: AgentErrorCode::Io,
            message: format!("Failed to inspect the project workspace root: {error}"),
            agent_id: None,
            installation_id: Some(context.installation_id.clone()),
            resource: None,
            retryable: true,
            details: Some(serde_json::json!({"phase": "execution_confinement"})),
        })
}

pub(crate) fn validate_project_workspace_root(project: &Path) -> Result<(), String> {
    if !project.is_absolute() {
        return Err(format!(
            "Project workspace root is not absolute: {}",
            project.display()
        ));
    }
    let mut managed_roots = vec![
        ad_home().map_err(|error| error.to_string())?,
        claude_dir().map_err(|error| error.to_string())?,
        codex_dir().map_err(|error| error.to_string())?,
    ];
    if let Ok(environment_home) = std::env::var("CODEX_HOME") {
        managed_roots.push(PathBuf::from(environment_home));
    }
    for configured in managed_roots {
        for managed in [
            configured.clone(),
            std::fs::canonicalize(&configured).unwrap_or(configured),
        ] {
            if project == managed || project.starts_with(&managed) || managed.starts_with(project) {
                return Err(format!(
                    "Project workspace root overlaps an Agent or AD managed root: {}",
                    project.display()
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct ConfinedTarget {
    parent: OwnedFd,
    pending_parents: Vec<OsString>,
    name: OsString,
    resource: ResourceRef,
    display_path: PathBuf,
}

impl ConfinedTarget {
    pub(super) fn observe_dependency_bound(
        resource: &ResourceRef,
        target: &Path,
        project_root_identity: Option<ProjectRootIdentity>,
    ) -> Result<TargetState, AgentError> {
        Self::resolve_internal(resource, target, true, project_root_identity)?.observe()
    }

    pub(super) fn observe_existing(
        resource: &ResourceRef,
        target: &Path,
    ) -> Result<TargetState, AgentError> {
        Self::resolve_internal(resource, target, false, None)?.observe()
    }

    #[cfg(test)]
    pub(super) fn resolve(resource: &ResourceRef, target: &Path) -> Result<Self, AgentError> {
        Self::resolve_internal(resource, target, false, None)
    }

    pub(super) fn resolve_bound(
        resource: &ResourceRef,
        target: &Path,
        project_root_identity: Option<ProjectRootIdentity>,
    ) -> Result<Self, AgentError> {
        Self::resolve_internal(resource, target, false, project_root_identity)
    }

    fn resolve_internal(
        resource: &ResourceRef,
        target: &Path,
        allow_dependency_root: bool,
        project_root_identity: Option<ProjectRootIdentity>,
    ) -> Result<Self, AgentError> {
        let root = if allow_dependency_root {
            trusted_observation_root(resource, target, project_root_identity)?
        } else {
            trusted_root(resource, target, project_root_identity)?
        };
        let target_for_root = if target.starts_with(&root) {
            target.to_path_buf()
        } else {
            canonicalize_target_parent(target).ok_or_else(|| {
                confinement_error(
                    resource,
                    format!(
                        "Mutation target {} cannot be resolved beneath trusted root {}",
                        target.display(),
                        root.display()
                    ),
                )
            })?
        };
        let relative = target_for_root.strip_prefix(&root).map_err(|_| {
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
        let expected_identity = resource
            .project_path
            .as_deref()
            .filter(|project| Path::new(project) == root)
            .and(project_root_identity);
        let root_fd = open_trusted_directory(&root, resource, expected_identity)?;
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

    #[cfg(test)]
    pub(super) fn write_directory_atomic(&self, source: &Path) -> Result<(), AgentError> {
        self.write_directory_atomic_filtered(source, false)
    }

    pub(super) fn write_directory_atomic_filtered(
        &self,
        source: &Path,
        exclude_agent_skill_projections: bool,
    ) -> Result<(), AgentError> {
        let parent = self
            .parent(true)?
            .expect("creating target parents always returns a directory");
        super::execution_tree::write_directory_atomic(
            &parent,
            self.name.as_os_str(),
            source,
            exclude_agent_skill_projections,
        )
        .map_err(|error| self.io_error(error))
    }

    pub(super) fn write_directory_from(&self, source: &StateDirectory) -> Result<(), AgentError> {
        let parent = self
            .parent(true)?
            .expect("creating target parents always returns a directory");
        super::execution_tree::write_directory_atomic_from(
            &parent,
            self.name.as_os_str(),
            source.fd(),
        )
        .map_err(|error| self.io_error(error))
    }

    pub(super) fn copy_directory_to(
        &self,
        destination: &StateDirectory,
        name: &str,
    ) -> Result<(), AgentError> {
        let parent = self.parent(false)?.ok_or_else(|| {
            self.io_error(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Directory target parent no longer exists",
            ))
        })?;
        super::execution_tree::copy_directory_to(
            &parent,
            self.name.as_os_str(),
            destination.fd(),
            OsStr::new(name),
        )
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

fn canonicalize_target_parent(target: &Path) -> Option<PathBuf> {
    let mut existing = target.parent()?;
    let mut suffix = vec![target.file_name()?.to_os_string()];
    while !existing.exists() {
        suffix.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?;
    }
    let mut normalized = std::fs::canonicalize(existing).ok()?;
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    Some(normalized)
}

fn trusted_root(
    resource: &ResourceRef,
    target: &Path,
    project_root_identity: Option<ProjectRootIdentity>,
) -> Result<PathBuf, AgentError> {
    if let Some(root) = trusted_scoped_root(resource, target, project_root_identity)? {
        return Ok(root);
    }
    trusted_user_agent_root(resource, target)
}

fn trusted_observation_root(
    resource: &ResourceRef,
    target: &Path,
    project_root_identity: Option<ProjectRootIdentity>,
) -> Result<PathBuf, AgentError> {
    if let Some(root) = trusted_scoped_root(resource, target, project_root_identity)? {
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
            open_configured_agent_root(&candidate, resource)?;
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
    project_root_identity: Option<ProjectRootIdentity>,
) -> Result<Option<PathBuf>, AgentError> {
    let ad_root = ad_home().map_err(|error| confinement_error(resource, error.to_string()))?;
    let canonical_ad_root = std::fs::canonicalize(&ad_root).ok();
    if target.starts_with(&ad_root)
        || canonical_ad_root
            .as_ref()
            .is_some_and(|root| target.starts_with(root))
    {
        ExecutionState::open().map_err(|error| confinement_error(resource, error.to_string()))?;
        return std::fs::canonicalize(&ad_root)
            .map(Some)
            .map_err(|error| confinement_error(resource, error.to_string()));
    }
    if let Some(project_path) = resource.project_path.as_deref() {
        let project = PathBuf::from(project_path);
        if target.starts_with(&project) {
            validate_project_workspace_root(&project)
                .map_err(|message| confinement_error(resource, message))?;
            open_trusted_directory(&project, resource, project_root_identity)?;
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
            open_configured_agent_root(&candidate, resource)?;
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

fn open_configured_agent_root(
    configured: &Path,
    resource: &ResourceRef,
) -> Result<OwnedFd, AgentError> {
    let parent = configured
        .parent()
        .ok_or_else(|| confinement_error(resource, "Agent root has no parent"))?;
    let name = configured
        .file_name()
        .ok_or_else(|| confinement_error(resource, "Agent root has no name"))?;
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|error| confinement_error(resource, error.to_string()))?;
    open_trusted_directory(&canonical_parent.join(name), resource, None)
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

fn open_trusted_directory(
    root: &Path,
    resource: &ResourceRef,
    expected_identity: Option<ProjectRootIdentity>,
) -> Result<OwnedFd, AgentError> {
    let fd = open_absolute_directory_nofollow(root)
        .map_err(|error| confinement_error(resource, error.to_string()))?;
    validate_directory_fd(&fd, root).map_err(|error| confinement_error(resource, error))?;
    if expected_identity.is_some_and(|expected| {
        project_root_identity(&fd)
            .map(|actual| actual != expected)
            .unwrap_or(true)
    }) {
        return Err(confinement_error(
            resource,
            "Project workspace root identity changed after preview",
        ));
    }
    Ok(fd)
}

fn open_absolute_directory_nofollow(root: &Path) -> Result<OwnedFd, std::io::Error> {
    if !root.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Trusted root is not absolute: {}", root.display()),
        ));
    }
    let mut current = open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty())?;
    for component in root.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                current = openat(&current, name, DIRECTORY_FLAGS, Mode::empty())?;
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Trusted root contains an unsafe component: {}",
                        root.display()
                    ),
                ))
            }
        }
    }
    Ok(current)
}

fn project_root_identity(fd: &OwnedFd) -> Result<ProjectRootIdentity, std::io::Error> {
    let stat = fstat(fd)?;
    Ok(ProjectRootIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
    })
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

use std::path::{Path, PathBuf};

use super::{AgentError, AgentErrorCode, ContentDigest, ManagedResourceTarget, PlannedMutation};

#[derive(Clone)]
pub(super) enum TargetState {
    Missing,
    File(Vec<u8>),
    Symlink(PathBuf),
}

impl TargetState {
    pub(super) fn digest(&self) -> Option<ContentDigest> {
        match self {
            Self::Missing => None,
            Self::File(bytes) => Some(ContentDigest::sha256(bytes)),
            Self::Symlink(target) => {
                Some(ContentDigest::sha256(target.to_string_lossy().as_bytes()))
            }
        }
    }
}

pub(super) fn observe_target(target: &ManagedResourceTarget) -> Result<TargetState, AgentError> {
    match std::fs::symlink_metadata(target.path()) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(target.path())
            .map(TargetState::Symlink)
            .map_err(|error| standalone_io_error(target.path(), error)),
        Ok(metadata) if metadata.is_file() => std::fs::read(target.path())
            .map(TargetState::File)
            .map_err(|error| standalone_io_error(target.path(), error)),
        Ok(_) => Err(AgentError {
            code: AgentErrorCode::PermissionDenied,
            message: format!(
                "Managed target is not a file or symlink: {}",
                target.path().display()
            ),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: None,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TargetState::Missing),
        Err(error) => Err(standalone_io_error(target.path(), error)),
    }
}

pub(super) fn render_content(mutation: &PlannedMutation) -> Result<Vec<u8>, AgentError> {
    let content = mutation.content.as_ref().ok_or_else(|| AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: "Create and replace mutations require content".into(),
        agent_id: None,
        installation_id: Some(mutation.resource.installation_id.clone()),
        resource: Some(mutation.resource.clone()),
        retryable: false,
        details: None,
    })?;
    if mutation.media_type == "application/json" {
        serde_json::to_vec_pretty(content).map_err(|error| content_error(mutation, error))
    } else if let Some(value) = content.as_str() {
        Ok(value.as_bytes().to_vec())
    } else {
        serde_json::to_vec(content).map_err(|error| content_error(mutation, error))
    }
}

pub(super) fn remove_target(path: &Path) -> Result<(), AgentError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(standalone_io_error(path, error)),
    }
}

pub(super) fn write_symlink_atomic(target: &Path, source: &Path) -> Result<(), std::io::Error> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Symlink target has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("link");
    let temporary = parent.join(format!(".{name}.tmp.{}", uuid::Uuid::new_v4().simple()));
    std::os::unix::fs::symlink(source, &temporary)?;
    if let Err(error) = std::fs::rename(&temporary, target) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn content_error(mutation: &PlannedMutation, error: serde_json::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: error.to_string(),
        agent_id: None,
        installation_id: Some(mutation.resource.installation_id.clone()),
        resource: Some(mutation.resource.clone()),
        retryable: false,
        details: None,
    }
}

fn standalone_io_error(path: &Path, error: std::io::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: format!("Failed to access {}: {error}", path.display()),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: true,
        details: None,
    }
}

use std::io::ErrorKind;
use std::path::PathBuf;

use crate::fs::paths::claude_dir;

use super::super::{
    AgentContext, AgentError, AgentErrorCode, AgentId, DiscoveryEvidence, InstallationCandidate,
    ResourceRef,
};

pub(super) fn resolve_claude_home(context: &AgentContext) -> Result<PathBuf, AgentError> {
    let path = claude_dir().map_err(|error| {
        agent_error(
            AgentErrorCode::Io,
            context,
            None,
            format!("Failed to resolve Claude config home: {error}"),
        )
    })?;
    let candidate = InstallationCandidate::from_existing_home(
        "claude-code",
        path,
        DiscoveryEvidence::DefaultHome,
    )
    .ok_or_else(|| {
        agent_error(
            AgentErrorCode::Unsupported,
            context,
            None,
            "Claude config home is not available",
        )
    })?;
    if candidate.installation().id != context.installation_id {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Agent context does not target the discovered Claude installation",
        ));
    }
    Ok(PathBuf::from(&candidate.installation().root_path))
}

pub(super) fn validate_project_path(
    context: &AgentContext,
    project_path: &str,
) -> Result<PathBuf, AgentError> {
    let canonical = std::fs::canonicalize(project_path).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid project path {project_path}: {error}"),
        )
    })?;
    if !canonical.is_dir() || canonical.to_string_lossy() != project_path {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Agent context project path is not canonical",
        ));
    }
    Ok(canonical)
}

pub(super) fn read_optional(
    path: &PathBuf,
    context: &AgentContext,
    resource: Option<ResourceRef>,
) -> Result<Option<Vec<u8>>, AgentError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(agent_error(
            AgentErrorCode::Io,
            context,
            resource,
            format!("Failed to read {}: {error}", path.display()),
        )),
    }
}

pub(super) fn agent_error(
    code: AgentErrorCode,
    context: &AgentContext,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: Some(AgentId::from("claude-code")),
        installation_id: Some(context.installation_id.clone()),
        resource,
        retryable: false,
        details: None,
    }
}

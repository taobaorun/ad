use std::path::Path;

use super::{
    builtin_registry, opaque_contract_id, project_runtime_descriptor_for_base_project,
    project_runtime_descriptor_for_context, runtime_for_installation, AgentError, AgentErrorCode,
    AgentInstallation, ContentDigest, InstallationId, ProjectCodexRuntimeDescriptor,
    ProjectRuntimeIdentity, WorkspaceDescriptor, WorkspaceRevision,
};

pub fn resolve_project_agent_workspace(
    installation_id: &InstallationId,
    project_path: &Path,
) -> Result<WorkspaceDescriptor, AgentError> {
    let canonical_project = std::fs::canonicalize(project_path).map_err(|error| {
        workspace_error(
            installation_id,
            format!("Invalid project path {}: {error}", project_path.display()),
        )
    })?;
    if !canonical_project.is_dir() {
        return Err(workspace_error(
            installation_id,
            format!(
                "Project path is not a directory: {}",
                project_path.display()
            ),
        ));
    }
    let canonical_project_path = canonical_project.to_string_lossy().into_owned();
    if let Some(runtime) = runtime_for_installation(installation_id) {
        if runtime.project_path != canonical_project_path {
            return Err(workspace_error(
                installation_id,
                "Project Codex runtime belongs to a different project",
            ));
        }
    }

    let registry = builtin_registry();
    let installations = registry.discover();
    let selected = installations
        .iter()
        .find(|installation| installation.id == *installation_id)
        .cloned();
    let runtime = resolve_runtime(installation_id, &canonical_project, selected.as_ref())?;
    let base_installation_id = runtime
        .as_ref()
        .map(|runtime| runtime.base_installation_id.clone())
        .or_else(|| {
            selected
                .as_ref()
                .and_then(|installation| installation.base_installation_id.clone())
        })
        .unwrap_or_else(|| installation_id.clone());
    let base = installations
        .iter()
        .find(|installation| installation.id == base_installation_id)
        .cloned()
        .or_else(|| {
            selected
                .clone()
                .filter(|installation| installation.base_installation_id.is_none())
        })
        .ok_or_else(|| workspace_error(installation_id, "Unknown Agent installation"))?;
    if let Some(selected) = selected.as_ref() {
        if let Some(selected_project) = selected.project_path.as_deref() {
            if selected_project != canonical_project_path {
                return Err(workspace_error(
                    installation_id,
                    "Project Agent installation belongs to a different project",
                ));
            }
        }
        if selected.agent_id != base.agent_id {
            return Err(workspace_error(
                installation_id,
                "Project runtime and base installation use different Agent adapters",
            ));
        }
    }

    let prepared_runtime = runtime.filter(runtime_is_prepared);
    let runtime_identity = prepared_runtime
        .as_ref()
        .map(project_runtime_identity)
        .transpose()?;
    Ok(WorkspaceDescriptor::for_installation(
        &canonical_project_path,
        &base,
        runtime_identity,
    ))
}

fn resolve_runtime(
    installation_id: &InstallationId,
    project_path: &Path,
    selected: Option<&AgentInstallation>,
) -> Result<Option<ProjectCodexRuntimeDescriptor>, AgentError> {
    if selected.is_some_and(|installation| installation.agent_id.as_str() != "codex") {
        return Ok(None);
    }
    let result = if selected.is_some_and(|installation| installation.base_installation_id.is_none())
    {
        project_runtime_descriptor_for_base_project(installation_id, project_path)
    } else {
        project_runtime_descriptor_for_context(installation_id, project_path)
    };
    result.map_err(|error| workspace_error(installation_id, error.to_string()))
}

fn runtime_is_prepared(runtime: &ProjectCodexRuntimeDescriptor) -> bool {
    std::fs::symlink_metadata(runtime.runtime_home.join("config.toml"))
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn project_runtime_identity(
    runtime: &ProjectCodexRuntimeDescriptor,
) -> Result<ProjectRuntimeIdentity, AgentError> {
    let config_digest = digest_file(&runtime.runtime_home.join("config.toml"))?;
    let manifest_digest = digest_file(&runtime.manifest_path())?;
    let revision = WorkspaceRevision::from(opaque_contract_id(
        "runtime-revision",
        &[
            runtime.runtime_installation_id.as_str(),
            runtime.base_installation_id.as_str(),
            config_digest.as_str(),
            manifest_digest.as_str(),
        ],
    ));
    Ok(ProjectRuntimeIdentity {
        installation_id: runtime.runtime_installation_id.clone(),
        base_installation_id: runtime.base_installation_id.clone(),
        revision,
    })
}

fn digest_file(path: &Path) -> Result<ContentDigest, AgentError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(ContentDigest::sha256(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ContentDigest::sha256(&[]))
        }
        Err(error) => Err(AgentError {
            code: AgentErrorCode::Io,
            message: format!(
                "Failed to read Project Runtime state at {}: {error}",
                path.display()
            ),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: true,
            details: Some(serde_json::json!({"phase": "workspace_resolution"})),
        }),
    }
}

fn workspace_error(installation_id: &InstallationId, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: None,
        installation_id: Some(installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "workspace_resolution"})),
    }
}

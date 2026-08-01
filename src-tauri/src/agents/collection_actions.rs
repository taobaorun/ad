use std::path::Path;

use super::collection_skills::project_ownership_records;
use super::{
    builtin_registry, inspect_project_workspace_inventory, load_skill_catalog_snapshot,
    verify_skill_artifact, AgentContext, AgentError, AgentErrorCode, CollectionInstallRequest,
    CollectionResourceView, ExecutionEngine, InstallationId, MutationPlan, OperationStatus,
    PlanAcknowledgement, PlanAcknowledgementCode, PlanId, PlanStore,
    ProjectCollectionActionPreview, ProjectCollectionActionRequest, ProjectWorkspaceInventory,
    ResourceAction, ResourceActionAvailability, ResourceKey, ResourceKind, ResourceRef,
    ResourceScope, RiskFingerprint, WorkspaceDescriptor, WorkspaceOperationIssue,
    WorkspaceOperationOutcome, WorkspaceOperationReport,
};

pub(crate) struct PlannedProjectCollectionAction {
    pub(crate) workspace: WorkspaceDescriptor,
    pub(crate) plan: MutationPlan,
}

pub(crate) fn plan_project_collection_action(
    installation_id: &InstallationId,
    project_path: &Path,
    request: &ProjectCollectionActionRequest,
) -> Result<PlannedProjectCollectionAction, AgentError> {
    let inventory = inspect_project_workspace_inventory(installation_id, project_path)?;
    validate_request(&inventory, request)?;
    let resource = inventory
        .skills
        .resources
        .iter()
        .chain(inventory.plugins.resources.iter())
        .find(|resource| resource.key == request.resource_key)
        .ok_or_else(|| action_error(&inventory.workspace, "Workspace resource no longer exists"))?;
    let action = resource
        .management
        .actions
        .iter()
        .find(|candidate| candidate.action == request.action)
        .ok_or_else(|| action_error(&inventory.workspace, "Resource action was not offered"))?;
    if !matches!(
        action.availability,
        ResourceActionAvailability::Available | ResourceActionAvailability::ConfirmationRequired
    ) {
        return Err(action_error(
            &inventory.workspace,
            "Resource action is unavailable",
        ));
    }
    let context = AgentContext {
        installation_id: inventory.workspace.effective_installation_id.clone(),
        project_path: Some(inventory.workspace.canonical_project_path.clone()),
    };
    let registry = builtin_registry();
    let adapter = registry.adapter_for_context(&context)?;
    let plan = match resource.kind {
        ResourceKind::Skills => {
            let port = adapter
                .skills()
                .ok_or_else(|| action_error(&inventory.workspace, "Agent has no Skill port"))?;
            match request.action {
                ResourceAction::Install => {
                    let binding = catalog_binding(&inventory.workspace, &resource.key)?;
                    port.plan_install(&context, binding.request)?
                }
                ResourceAction::Update => {
                    let binding = catalog_binding(&inventory.workspace, &resource.key)?;
                    let installed = owned_skill(&inventory.workspace, resource)?;
                    port.plan_update(&context, &installed, binding.request)?
                }
                ResourceAction::Remove => {
                    let installed = owned_skill(&inventory.workspace, resource)?;
                    port.plan_remove(&context, &installed)?
                }
                ResourceAction::Enable | ResourceAction::Disable => {
                    let installed = owned_skill(&inventory.workspace, resource)?;
                    port.plan_set_enabled(
                        &context,
                        &installed,
                        request.action == ResourceAction::Enable,
                    )?
                }
                _ => {
                    return Err(action_error(
                        &inventory.workspace,
                        "Unsupported Skill action",
                    ))
                }
            }
        }
        ResourceKind::Plugins => {
            let port = adapter
                .plugins()
                .ok_or_else(|| action_error(&inventory.workspace, "Agent has no Plugin port"))?;
            let target = ResourceRef {
                installation_id: context.installation_id.clone(),
                project_path: context.project_path.clone(),
                kind: ResourceKind::Plugins,
                scope: ResourceScope::Project,
                logical_id: resource.logical_id.clone(),
            };
            match request.action {
                ResourceAction::Enable | ResourceAction::Disable => port.plan_set_enabled(
                    &context,
                    &target,
                    request.action == ResourceAction::Enable,
                )?,
                ResourceAction::Remove => port.plan_remove(&context, &target)?,
                _ => {
                    return Err(action_error(
                        &inventory.workspace,
                        "Unsupported Plugin action",
                    ))
                }
            }
        }
        _ => {
            return Err(action_error(
                &inventory.workspace,
                "Only Skills and Plugins have collection actions",
            ))
        }
    };
    plan.validate()?;
    Ok(PlannedProjectCollectionAction {
        workspace: inventory.workspace,
        plan,
    })
}

pub fn preview_project_collection_action(
    installation_id: &InstallationId,
    project_path: &Path,
    request: ProjectCollectionActionRequest,
    plans: &PlanStore,
) -> Result<ProjectCollectionActionPreview, AgentError> {
    let planned = plan_project_collection_action(installation_id, project_path, &request)?;
    let plan = plans.insert_workspace_action(
        planned.plan,
        &planned.workspace.key,
        &request.resource_key,
        request.action,
    )?;
    Ok(ProjectCollectionActionPreview {
        workspace_key: planned.workspace.key,
        resource_key: request.resource_key,
        action: request.action,
        plan,
    })
}

pub fn apply_project_collection_action_plan(
    plan_id: &PlanId,
    expected_context: &AgentContext,
    expected_risk_fingerprint: &RiskFingerprint,
    confirmed: bool,
    plans: &PlanStore,
) -> Result<WorkspaceOperationReport, AgentError> {
    let receipt = ExecutionEngine.apply_acknowledged_bound(
        plan_id,
        expected_context,
        expected_risk_fingerprint,
        plans,
        &[PlanAcknowledgement {
            code: PlanAcknowledgementCode::ProjectCollectionApply,
            accepted: confirmed,
        }],
    )?;
    let workspace_key = receipt.workspace_key.clone().ok_or_else(|| AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: "Collection action receipt has no workspace identity".into(),
        agent_id: None,
        installation_id: Some(expected_context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "collection_action"})),
    })?;
    let (outcome, issues) = match receipt.status {
        OperationStatus::Complete => (WorkspaceOperationOutcome::Changed, Vec::new()),
        OperationStatus::Compensated => (
            WorkspaceOperationOutcome::PartialFailure,
            vec![operation_issue("collection_action_compensated")],
        ),
        OperationStatus::PartialFailure => (
            WorkspaceOperationOutcome::PartialFailure,
            vec![operation_issue("collection_action_partial_failure")],
        ),
    };
    Ok(WorkspaceOperationReport {
        workspace_key,
        outcome,
        issues,
        receipt: Some(receipt),
    })
}

fn operation_issue(code: &str) -> WorkspaceOperationIssue {
    let message_key = match code {
        "collection_action_compensated" => "agents.resources.actionCompensated",
        "collection_action_partial_failure" => "agents.resources.actionPartialFailure",
        _ => "agents.resources.actionFailed",
    };
    WorkspaceOperationIssue {
        code: code.into(),
        message_key: message_key.into(),
        resource_key: None,
    }
}

struct CatalogBinding {
    request: CollectionInstallRequest,
}

fn catalog_binding(
    workspace: &WorkspaceDescriptor,
    resource_key: &ResourceKey,
) -> Result<CatalogBinding, AgentError> {
    let catalog = load_skill_catalog_snapshot()
        .map_err(|error| action_error(workspace, error.to_string()))?;
    for source in catalog.entries {
        for skill in &source.current_artifact.skills {
            let key = ResourceKey::for_collection(
                &workspace.key,
                &workspace.agent_id,
                ResourceKind::Skills,
                &skill.logical_id,
                &source.source_id,
            );
            if &key == resource_key {
                let tree = verify_skill_artifact(&source.current_artifact)
                    .map_err(|error| action_error(workspace, error.to_string()))?;
                return Ok(CatalogBinding {
                    request: CollectionInstallRequest {
                        logical_id: format!("{}/{}", source.source_id, skill.logical_id),
                        source: serde_json::json!({
                            "artifactId": source.current_artifact.artifact_id,
                            "path": tree.join(&skill.subpath),
                        }),
                    },
                });
            }
        }
    }
    Err(action_error(
        workspace,
        "Catalog Skill revision is unavailable",
    ))
}

fn owned_skill(
    workspace: &WorkspaceDescriptor,
    view: &CollectionResourceView,
) -> Result<ResourceRef, AgentError> {
    let record_id = view.ownership.record_id.as_ref().ok_or_else(|| {
        action_error(
            workspace,
            "Skill ownership is not proven for this workspace",
        )
    })?;
    project_ownership_records(workspace)?
        .into_iter()
        .find(|record| &record.id == record_id)
        .map(|record| record.resource)
        .ok_or_else(|| action_error(workspace, "Skill ownership record changed"))
}

fn validate_request(
    inventory: &ProjectWorkspaceInventory,
    request: &ProjectCollectionActionRequest,
) -> Result<(), AgentError> {
    if inventory.workspace.key != request.workspace_key
        || inventory.revision != request.inventory_revision
    {
        Err(action_error(
            &inventory.workspace,
            "Workspace inventory changed after the action was shown",
        ))
    } else {
        Ok(())
    }
}

fn action_error(workspace: &WorkspaceDescriptor, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::ResourceChanged,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.effective_installation_id.clone()),
        resource: None,
        retryable: true,
        details: Some(serde_json::json!({"phase": "collection_action"})),
    }
}

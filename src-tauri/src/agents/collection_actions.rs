use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::collection_skills::project_ownership_records;
use super::{
    builtin_registry, inspect_project_workspace_inventory, AgentContext, AgentError,
    AgentErrorCode, CollectionInstallRequest, CollectionResourceView, ExecutionEngine,
    InstallationId, MutationPlan, OperationStatus, PlanAcknowledgement, PlanAcknowledgementCode,
    PlanId, PlanStore, ProjectCollectionActionPreview, ProjectCollectionActionRequest,
    ProjectCollectionSourceInstallPreview, ProjectCollectionSourceInstallRequest,
    ProjectWorkspaceInventory, ReadPrecondition, ResourceAction, ResourceActionAvailability,
    ResourceKey, ResourceKind, ResourceRef, ResourceScope, ResourceSourceKind, RiskFingerprint,
    WorkspaceDescriptor, WorkspaceOperationIssue, WorkspaceOperationOutcome,
    WorkspaceOperationReport,
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
    plan_inventory_collection_action(&inventory, &request.resource_key, request.action)
}

fn plan_inventory_collection_action(
    inventory: &ProjectWorkspaceInventory,
    resource_key: &ResourceKey,
    requested_action: ResourceAction,
) -> Result<PlannedProjectCollectionAction, AgentError> {
    let resource = inventory
        .skills
        .resources
        .iter()
        .chain(inventory.plugins.resources.iter())
        .find(|resource| resource.key == *resource_key)
        .ok_or_else(|| action_error(&inventory.workspace, "Workspace resource no longer exists"))?;
    let action = resource
        .management
        .actions
        .iter()
        .find(|candidate| candidate.action == requested_action)
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
            match requested_action {
                ResourceAction::Install => {
                    let resource_id = catalog_resource_for_key(
                        &inventory.workspace,
                        &resource.key,
                        ResourceKind::Skills,
                    )?;
                    port.plan_install(&context, CollectionInstallRequest::catalog(resource_id))?
                }
                ResourceAction::Update => {
                    let resource_id = catalog_resource_for_key(
                        &inventory.workspace,
                        &resource.key,
                        ResourceKind::Skills,
                    )?;
                    let installed = owned_skill(&inventory.workspace, resource)?;
                    port.plan_update(
                        &context,
                        &installed,
                        CollectionInstallRequest::catalog(resource_id),
                    )?
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
                        requested_action == ResourceAction::Enable,
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
            match requested_action {
                ResourceAction::Install => {
                    let resource_id = catalog_resource_for_key(
                        &inventory.workspace,
                        &resource.key,
                        ResourceKind::Plugins,
                    )?;
                    port.plan_install(&context, CollectionInstallRequest::catalog(resource_id))?
                }
                ResourceAction::Enable | ResourceAction::Disable => {
                    let target = owned_collection(&inventory.workspace, resource)?;
                    port.plan_set_enabled(
                        &context,
                        &target,
                        requested_action == ResourceAction::Enable,
                    )?
                }
                ResourceAction::Remove => {
                    let target = if resource.ownership.record_id.is_some() {
                        owned_collection(&inventory.workspace, resource)?
                    } else {
                        ResourceRef {
                            installation_id: context.installation_id.clone(),
                            project_path: context.project_path.clone(),
                            kind: ResourceKind::Plugins,
                            scope: ResourceScope::Project,
                            logical_id: resource.logical_id.clone(),
                        }
                    };
                    port.plan_remove(&context, &target)?
                }
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
        workspace: inventory.workspace.clone(),
        plan,
    })
}

fn catalog_resource_for_key(
    workspace: &WorkspaceDescriptor,
    resource_key: &ResourceKey,
    kind: ResourceKind,
) -> Result<String, AgentError> {
    let catalog = super::load_resource_catalog_snapshot()
        .map_err(|error| action_error(workspace, error.to_string()))?;
    catalog
        .resources
        .values()
        .find(|candidate| {
            candidate.kind == kind
                && candidate.present
                && candidate.lifecycle == super::ResourceLifecycle::Managed
                && ResourceKey::for_collection(
                    &workspace.key,
                    &workspace.agent_id,
                    kind,
                    &candidate.install_id,
                    &candidate.source_id,
                ) == *resource_key
        })
        .map(|candidate| candidate.id.clone())
        .ok_or_else(|| action_error(workspace, "Catalog resource revision is unavailable"))
}

fn owned_collection(
    workspace: &WorkspaceDescriptor,
    view: &CollectionResourceView,
) -> Result<ResourceRef, AgentError> {
    let record_id = view.ownership.record_id.as_ref().ok_or_else(|| {
        action_error(
            workspace,
            "Collection ownership is not proven for this workspace",
        )
    })?;
    super::collection_skills::project_ownership_records_for(workspace, view.kind)?
        .into_iter()
        .find(|record| &record.id == record_id)
        .map(|record| record.resource)
        .ok_or_else(|| action_error(workspace, "Collection ownership record changed"))
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

pub fn preview_project_collection_source_install(
    installation_id: &InstallationId,
    project_path: &Path,
    request: ProjectCollectionSourceInstallRequest,
    plans: &PlanStore,
) -> Result<ProjectCollectionSourceInstallPreview, AgentError> {
    let inventory = inspect_project_workspace_inventory(installation_id, project_path)?;
    validate_source_install_request(&inventory, &request)?;
    let source = inventory
        .skills
        .resources
        .iter()
        .find(|resource| resource.key == request.source_resource_key)
        .and_then(|resource| resource.provenance.source.as_ref())
        .filter(|source| {
            matches!(
                source.kind,
                ResourceSourceKind::CatalogGit | ResourceSourceKind::CatalogLocal
            )
        })
        .cloned()
        .ok_or_else(|| {
            action_error(
                &inventory.workspace,
                "Skill source is no longer available for batch installation",
            )
        })?;
    let resource_keys = inventory
        .skills
        .resources
        .iter()
        .filter(|resource| resource.provenance.source.as_ref() == Some(&source))
        .filter(|resource| action_available(resource, ResourceAction::Install))
        .map(|resource| resource.key.clone())
        .collect::<Vec<_>>();
    if resource_keys.is_empty() {
        return Err(action_error(
            &inventory.workspace,
            "Skill source has no installable resources",
        ));
    }

    let mut partial_plans = Vec::with_capacity(resource_keys.len());
    for resource_key in &resource_keys {
        partial_plans.push(
            plan_inventory_collection_action(&inventory, resource_key, ResourceAction::Install)?
                .plan,
        );
    }
    let combined = combine_collection_plans(&inventory.workspace, partial_plans)?;
    let plan = plans.insert_workspace_source_install(
        combined,
        &inventory.workspace.key,
        &request.source_resource_key,
    )?;
    Ok(ProjectCollectionSourceInstallPreview {
        workspace_key: inventory.workspace.key,
        source,
        resource_keys,
        plan,
    })
}

fn action_available(resource: &CollectionResourceView, action: ResourceAction) -> bool {
    resource.management.actions.iter().any(|candidate| {
        candidate.action == action
            && matches!(
                candidate.availability,
                ResourceActionAvailability::Available
                    | ResourceActionAvailability::ConfirmationRequired
            )
    })
}

fn combine_collection_plans(
    workspace: &WorkspaceDescriptor,
    plans: Vec<MutationPlan>,
) -> Result<MutationPlan, AgentError> {
    let first = plans
        .first()
        .ok_or_else(|| action_error(workspace, "No installable Skill plans were produced"))?;
    let agent_id = first.agent_id.clone();
    let context = first.context.clone();
    let expires_at = plans
        .iter()
        .map(|plan| plan.expires_at)
        .min()
        .ok_or_else(|| action_error(workspace, "No installable Skill plans were produced"))?;
    let mut read_set = BTreeMap::<ResourceRef, ReadPrecondition>::new();
    let mut mutation_resources = BTreeSet::<ResourceRef>::new();
    let mut mutations = Vec::new();

    for plan in plans {
        if plan.agent_id != agent_id || plan.context != context {
            return Err(action_error(
                workspace,
                "Skill source produced plans for different Agent contexts",
            ));
        }
        for precondition in plan.read_set {
            if let Some(existing) = read_set.get(&precondition.resource) {
                if existing != &precondition {
                    return Err(action_error(
                        workspace,
                        "Skill source produced conflicting read preconditions",
                    ));
                }
            } else {
                read_set.insert(precondition.resource.clone(), precondition);
            }
        }
        for mutation in plan.mutations {
            if !mutation_resources.insert(mutation.resource.clone()) {
                return Err(action_error(
                    workspace,
                    "Skill source produced overlapping mutations",
                ));
            }
            mutations.push(mutation);
        }
    }

    let combined = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id,
        context,
        read_set: read_set.into_values().collect(),
        mutations,
        expires_at,
    };
    combined.validate()?;
    Ok(combined)
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

fn validate_source_install_request(
    inventory: &ProjectWorkspaceInventory,
    request: &ProjectCollectionSourceInstallRequest,
) -> Result<(), AgentError> {
    if inventory.workspace.key != request.workspace_key
        || inventory.revision != request.inventory_revision
    {
        Err(action_error(
            &inventory.workspace,
            "Workspace inventory changed after the source action was shown",
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

use std::collections::BTreeMap;

use super::user_inventory::user_ownership_records_for;
use super::{
    builtin_registry, inspect_user_resource_inventory, load_resource_catalog_snapshot,
    AgentContext, AgentError, AgentErrorCode, CollectionInstallRequest, CollectionResourceView,
    ExecutionEngine, InstallationId, MutationPlan, OperationStatus, PlanAcknowledgement,
    PlanAcknowledgementCode, PlanId, PlanStore, PlannedMutation, ReadPrecondition, ResourceAction,
    ResourceActionAvailability, ResourceKey, ResourceKind, ResourceRef, RiskFingerprint,
    UserCollectionActionPreview, UserCollectionActionRequest, UserCollectionSourceInstallPreview,
    UserCollectionSourceInstallRequest, UserPluginPlanStore, UserResourceInventory,
    UserWorkspaceDescriptor, WorkspaceOperationIssue, WorkspaceOperationOutcome,
    WorkspaceOperationReport,
};

pub fn preview_user_collection_action(
    installation_id: &InstallationId,
    request: UserCollectionActionRequest,
    plans: &PlanStore,
    plugin_plans: &UserPluginPlanStore,
) -> Result<UserCollectionActionPreview, AgentError> {
    let inventory = inspect_user_resource_inventory(installation_id)?;
    validate_request(
        &inventory,
        request.workspace_key.clone(),
        &request.inventory_revision,
    )?;
    let resource = inventory
        .skills
        .resources
        .iter()
        .chain(inventory.plugins.resources.iter())
        .find(|resource| resource.key == request.resource_key)
        .ok_or_else(|| action_error(&inventory.workspace, "User resource no longer exists"))?;
    ensure_action_available(&inventory.workspace, resource, request.action)?;
    let view = if resource.kind == ResourceKind::Plugins
        && !(inventory.workspace.agent_id.as_str() == "codex"
            && matches!(
                request.action,
                ResourceAction::Enable | ResourceAction::Disable
            )) {
        super::preview_user_plugin_action(
            &inventory.workspace,
            inventory.revision.clone(),
            request.resource_key.clone(),
            &catalog_resource_id(&inventory, resource)?,
            request.action,
            plugin_plans,
        )?
    } else {
        let plan = plan_user_action(&inventory, resource, request.action)?;
        plans.insert_user_collection_action(
            plan,
            &inventory.workspace.key,
            format!(
                "{}:{}",
                request.action.contract_name(),
                request.resource_key
            ),
        )?
    };
    Ok(UserCollectionActionPreview {
        workspace_key: inventory.workspace.key,
        resource_key: request.resource_key,
        action: request.action,
        plan: view,
    })
}

pub fn preview_user_collection_source_install(
    installation_id: &InstallationId,
    request: UserCollectionSourceInstallRequest,
    plans: &PlanStore,
) -> Result<UserCollectionSourceInstallPreview, AgentError> {
    let inventory = inspect_user_resource_inventory(installation_id)?;
    validate_request(
        &inventory,
        request.workspace_key.clone(),
        &request.inventory_revision,
    )?;
    let source = inventory
        .skills
        .resources
        .iter()
        .find(|resource| resource.key == request.source_resource_key)
        .and_then(|resource| resource.provenance.source.as_ref())
        .cloned()
        .ok_or_else(|| action_error(&inventory.workspace, "Skill source is no longer available"))?;
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
            "Skill source has no installable user resources",
        ));
    }
    let partial = resource_keys
        .iter()
        .map(|key| {
            let resource = inventory
                .skills
                .resources
                .iter()
                .find(|resource| &resource.key == key)
                .expect("resource key came from this inventory");
            plan_user_action(&inventory, resource, ResourceAction::Install)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let combined = combine_plans(&inventory.workspace, partial)?;
    let plan = plans.insert_user_collection_action(
        combined,
        &inventory.workspace.key,
        format!("install_source:{}", request.source_resource_key),
    )?;
    Ok(UserCollectionSourceInstallPreview {
        workspace_key: inventory.workspace.key,
        source,
        resource_keys,
        plan,
    })
}

pub async fn apply_user_collection_action_plan(
    plan_id: &PlanId,
    expected_context: &AgentContext,
    expected_risk_fingerprint: &RiskFingerprint,
    confirmed: bool,
    plans: &PlanStore,
    plugin_plans: &UserPluginPlanStore,
) -> Result<WorkspaceOperationReport, AgentError> {
    if expected_context.project_path.is_some() {
        return Err(AgentError {
            code: AgentErrorCode::InvalidPlan,
            message: "User collection apply requires a user Agent context".into(),
            agent_id: None,
            installation_id: Some(expected_context.installation_id.clone()),
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "user_collection_action"})),
        });
    }
    if plugin_plans.contains(plan_id) {
        return super::apply_user_plugin_plan(
            plan_id,
            expected_context,
            expected_risk_fingerprint,
            confirmed,
            plugin_plans,
        )
        .await;
    }
    let receipt = ExecutionEngine.apply_acknowledged_bound(
        plan_id,
        expected_context,
        expected_risk_fingerprint,
        plans,
        &[PlanAcknowledgement {
            code: PlanAcknowledgementCode::UserCollectionApply,
            accepted: confirmed,
        }],
    )?;
    let workspace_key = receipt.workspace_key.clone().ok_or_else(|| AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: "User collection receipt has no workspace identity".into(),
        agent_id: None,
        installation_id: Some(expected_context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "user_collection_action"})),
    })?;
    let (outcome, issues) = match receipt.status {
        OperationStatus::Complete => (WorkspaceOperationOutcome::Changed, Vec::new()),
        OperationStatus::Compensated | OperationStatus::PartialFailure => (
            WorkspaceOperationOutcome::PartialFailure,
            vec![WorkspaceOperationIssue {
                code: "user_collection_action_partial_failure".into(),
                message_key: "agents.resources.actionPartialFailure".into(),
                resource_key: None,
            }],
        ),
    };
    Ok(WorkspaceOperationReport {
        workspace_key,
        outcome,
        issues,
        receipt: Some(receipt),
    })
}

fn plan_user_action(
    inventory: &UserResourceInventory,
    resource: &CollectionResourceView,
    action: ResourceAction,
) -> Result<MutationPlan, AgentError> {
    let context = AgentContext {
        installation_id: inventory.workspace.installation_id.clone(),
        project_path: None,
    };
    let registry = builtin_registry();
    let adapter = registry.adapter_for_context(&context)?;
    match resource.kind {
        ResourceKind::Skills => {
            let port = adapter
                .skills()
                .ok_or_else(|| action_error(&inventory.workspace, "Agent has no Skill port"))?;
            match action {
                ResourceAction::Install => port.plan_install(
                    &context,
                    CollectionInstallRequest::catalog(catalog_resource_id(inventory, resource)?),
                ),
                ResourceAction::Update => {
                    let owned = owned_skill(&inventory.workspace, resource)?;
                    port.plan_update(
                        &context,
                        &owned,
                        CollectionInstallRequest::catalog(catalog_resource_id(
                            inventory, resource,
                        )?),
                    )
                }
                ResourceAction::Enable | ResourceAction::Disable => {
                    let owned = owned_skill(&inventory.workspace, resource)?;
                    port.plan_set_enabled(&context, &owned, action == ResourceAction::Enable)
                }
                ResourceAction::Remove => {
                    let owned = owned_skill(&inventory.workspace, resource)?;
                    port.plan_remove(&context, &owned)
                }
                _ => Err(action_error(
                    &inventory.workspace,
                    "Unsupported user Skill action",
                )),
            }
        }
        ResourceKind::Plugins => {
            if inventory.workspace.agent_id.as_str() != "codex"
                || !matches!(action, ResourceAction::Enable | ResourceAction::Disable)
            {
                return Err(action_error(
                    &inventory.workspace,
                    "User Plugin action requires the native Plugin executor",
                ));
            }
            let record_id = resource.ownership.record_id.as_ref().ok_or_else(|| {
                action_error(&inventory.workspace, "User Plugin is not AD-managed")
            })?;
            let record = super::user_plugin_record_by_id(&inventory.workspace, record_id)?
                .ok_or_else(|| {
                    action_error(&inventory.workspace, "User Plugin ownership changed")
                })?;
            let port = adapter
                .plugins()
                .ok_or_else(|| action_error(&inventory.workspace, "Agent has no Plugin port"))?;
            port.plan_set_enabled(
                &context,
                &ResourceRef {
                    installation_id: context.installation_id.clone(),
                    project_path: None,
                    kind: ResourceKind::Plugins,
                    scope: super::ResourceScope::User,
                    logical_id: record.native_id,
                },
                action == ResourceAction::Enable,
            )
        }
        _ => Err(action_error(
            &inventory.workspace,
            "Only Skills and Plugins have user collection actions",
        )),
    }
}

fn owned_skill(
    workspace: &UserWorkspaceDescriptor,
    view: &CollectionResourceView,
) -> Result<ResourceRef, AgentError> {
    let record_id = view
        .ownership
        .record_id
        .as_ref()
        .ok_or_else(|| action_error(workspace, "User Skill ownership is not proven"))?;
    user_ownership_records_for(workspace, ResourceKind::Skills)?
        .into_iter()
        .find(|record| &record.id == record_id)
        .map(|record| record.resource)
        .ok_or_else(|| action_error(workspace, "User Skill ownership record changed"))
}

fn catalog_resource_id(
    inventory: &UserResourceInventory,
    view: &CollectionResourceView,
) -> Result<String, AgentError> {
    let catalog = load_resource_catalog_snapshot()
        .map_err(|error| action_error(&inventory.workspace, error.to_string()))?;
    catalog
        .resources
        .values()
        .find(|candidate| {
            candidate.kind == view.kind
                && candidate.install_id == view.logical_id
                && candidate.present
                && ResourceKey::for_collection(
                    &inventory.workspace.key,
                    &inventory.workspace.agent_id,
                    view.kind,
                    &candidate.install_id,
                    &candidate.source_id,
                ) == view.key
        })
        .map(|candidate| candidate.id.clone())
        .ok_or_else(|| action_error(&inventory.workspace, "Catalog resource is unavailable"))
}

fn combine_plans(
    workspace: &UserWorkspaceDescriptor,
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
    let mut mutation_indexes = BTreeMap::<ResourceRef, usize>::new();
    let mut mutations = Vec::<PlannedMutation>::new();
    for plan in plans {
        if plan.agent_id != agent_id || plan.context != context {
            return Err(action_error(
                workspace,
                "User Skill plans disagree on Agent context",
            ));
        }
        for precondition in plan.read_set {
            read_set.insert(precondition.resource.clone(), precondition);
        }
        for mutation in plan.mutations {
            if let Some(index) = mutation_indexes.get(&mutation.resource).copied() {
                merge_skill_settings_mutation(workspace, &mut mutations[index], mutation)?;
                continue;
            }
            mutation_indexes.insert(mutation.resource.clone(), mutations.len());
            mutations.push(mutation);
        }
    }
    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id,
        context,
        read_set: read_set.into_values().collect(),
        mutations,
        expires_at,
    };
    plan.validate()?;
    Ok(plan)
}

fn merge_skill_settings_mutation(
    workspace: &UserWorkspaceDescriptor,
    existing: &mut PlannedMutation,
    incoming: PlannedMutation,
) -> Result<(), AgentError> {
    if existing.resource.kind != ResourceKind::Settings
        || incoming.resource.kind != ResourceKind::Settings
        || existing.kind != incoming.kind
        || existing.expected_digest != incoming.expected_digest
        || existing.media_type != "application/json"
        || incoming.media_type != "application/json"
    {
        return Err(action_error(workspace, "User Skill plans overlap"));
    }
    let existing_content = existing
        .content
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| action_error(workspace, "User Skill settings plan is invalid"))?;
    let incoming_content = incoming
        .content
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| action_error(workspace, "User Skill settings plan is invalid"))?;
    let mut existing_base = existing_content.clone();
    let mut incoming_base = incoming_content.clone();
    let existing_overrides = existing_base
        .remove("skillOverrides")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| action_error(workspace, "User Skill settings plan has no overrides"))?;
    let incoming_overrides = incoming_base
        .remove("skillOverrides")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| action_error(workspace, "User Skill settings plan has no overrides"))?;
    if existing_base != incoming_base {
        return Err(action_error(
            workspace,
            "User Skill settings plans disagree",
        ));
    }
    let mut combined = existing_overrides;
    for (name, value) in incoming_overrides {
        if combined
            .get(&name)
            .is_some_and(|existing| existing != &value)
        {
            return Err(action_error(
                workspace,
                "User Skill settings plans disagree",
            ));
        }
        combined.insert(name, value);
    }
    existing_content.insert("skillOverrides".into(), serde_json::Value::Object(combined));
    Ok(())
}

fn ensure_action_available(
    workspace: &UserWorkspaceDescriptor,
    resource: &CollectionResourceView,
    action: ResourceAction,
) -> Result<(), AgentError> {
    if action_available(resource, action) {
        Ok(())
    } else {
        Err(action_error(
            workspace,
            "User resource action is unavailable",
        ))
    }
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

fn validate_request(
    inventory: &UserResourceInventory,
    workspace_key: super::WorkspaceKey,
    revision: &super::InventoryRevision,
) -> Result<(), AgentError> {
    if inventory.workspace.key == workspace_key && &inventory.revision == revision {
        Ok(())
    } else {
        Err(action_error(
            &inventory.workspace,
            "User resource inventory changed after the action was shown",
        ))
    }
}

fn action_error(workspace: &UserWorkspaceDescriptor, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::ResourceChanged,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.installation_id.clone()),
        resource: None,
        retryable: true,
        details: Some(serde_json::json!({"phase": "user_collection_action"})),
    }
}

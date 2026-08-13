use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::{
    apply_skill_catalog_plan, builtin_registry, load_resource_catalog_snapshot, opaque_contract_id,
    resource_lifecycle_lock_target, set_resource_lifecycle, set_resource_lifecycle_under_lease,
    validate_ownership_target, AgentContext, AgentError, AgentErrorCode, ExecutionEngine,
    MutationPlanView, PlanId, PlanStore, ResourceInstallationId, ResourceInstallationRecord,
    ResourceKind, ResourceLifecycle, ResourceRef, ResourceScope, RiskFingerprint,
    SkillCatalogPlanClaim, SkillCatalogPlanStore, TargetLockSet, WorkspaceKey,
};

use super::resource_installations::list_resource_installations_for_lifecycle;

const REMOVAL_PLAN_TTL_MINUTES: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRemovalPhase {
    Uninstalling,
    Suppressing,
    Complete,
    PartialFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRemovalItemState {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRemovalInstallationView {
    pub installation_id: ResourceInstallationId,
    pub workspace_key: WorkspaceKey,
    pub agent_id: super::AgentId,
    pub project_path: String,
    pub state: ResourceRemovalItemState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRemovalPlanView {
    pub plan_id: PlanId,
    pub resource_id: String,
    pub resource_name: String,
    pub expected_catalog_revision: u64,
    pub affected_project_count: usize,
    pub affected_agent_count: usize,
    pub installations: Vec<ResourceRemovalInstallationView>,
    pub risk_fingerprint: RiskFingerprint,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRemovalProgress {
    pub operation_id: String,
    pub sequence: u64,
    pub phase: ResourceRemovalPhase,
    pub completed: usize,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ResourceRemovalInstallationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRemovalReport {
    pub operation_id: String,
    pub resource_id: String,
    pub phase: ResourceRemovalPhase,
    pub completed: usize,
    pub total: usize,
    pub installations: Vec<ResourceRemovalInstallationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRemovalResourceView {
    pub resource_id: String,
    pub resource_name: String,
    pub kind: ResourceKind,
    pub affected_project_count: usize,
    pub affected_agent_count: usize,
    pub state: ResourceRemovalItemState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRemovalPlanView {
    pub plan_id: PlanId,
    pub source_id: String,
    pub source_name: String,
    pub expected_catalog_revision: u64,
    pub affected_project_count: usize,
    pub affected_agent_count: usize,
    pub resources: Vec<SourceRemovalResourceView>,
    pub risk_fingerprint: RiskFingerprint,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRemovalPhase {
    Uninstalling,
    RemovingSource,
    Complete,
    PartialFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRemovalProgress {
    pub operation_id: String,
    pub sequence: u64,
    pub phase: SourceRemovalPhase,
    pub completed: usize,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<SourceRemovalResourceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRemovalReport {
    pub operation_id: String,
    pub source_id: String,
    pub phase: SourceRemovalPhase,
    pub completed: usize,
    pub total: usize,
    pub resources: Vec<SourceRemovalResourceView>,
}

struct StoredRemovalPlan {
    view: ResourceRemovalPlanView,
    installation_ids: Vec<ResourceInstallationId>,
}

struct ResourceRemovalOperationSeed {
    operation_id: String,
    started_at: DateTime<Utc>,
    installations: Vec<ResourceRemovalInstallationView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceRemovalOperationSnapshot {
    pub schema_version: u32,
    pub operation_id: String,
    pub resource_id: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub phase: ResourceRemovalPhase,
    pub completed: usize,
    pub total: usize,
    pub installations: Vec<ResourceRemovalInstallationView>,
}

#[derive(Default)]
pub struct ResourceRemovalPlanStore {
    plans: Mutex<HashMap<PlanId, StoredRemovalPlan>>,
    active_resources: Mutex<BTreeSet<String>>,
}

struct StoredSourceRemovalPlan {
    view: SourceRemovalPlanView,
    installation_ids: Vec<ResourceInstallationId>,
}

#[derive(Default)]
pub struct SourceRemovalPlanStore {
    plans: Mutex<HashMap<PlanId, StoredSourceRemovalPlan>>,
}

impl SourceRemovalPlanStore {
    pub fn preview(&self, source_id: &str) -> Result<SourceRemovalPlanView, AgentError> {
        let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
        let source = catalog
            .sources
            .get(source_id)
            .ok_or_else(|| lifecycle_error("Managed source was not found"))?;
        let installations = list_resource_installations_for_lifecycle().map_err(lifecycle_error)?;
        let mut resources = catalog
            .resources
            .values()
            .filter(|resource| resource.source_id == source_id)
            .map(|resource| {
                let matching = installations
                    .iter()
                    .filter(|installation| installation.resource_id == resource.id)
                    .collect::<Vec<_>>();
                SourceRemovalResourceView {
                    resource_id: resource.id.clone(),
                    resource_name: resource.display_name.clone(),
                    kind: resource.kind,
                    affected_project_count: matching
                        .iter()
                        .map(|item| &item.canonical_project_path)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    affected_agent_count: matching
                        .iter()
                        .map(|item| (&item.canonical_project_path, &item.agent_id))
                        .collect::<BTreeSet<_>>()
                        .len(),
                    state: if resource.lifecycle == ResourceLifecycle::Suppressed {
                        ResourceRemovalItemState::Succeeded
                    } else {
                        ResourceRemovalItemState::Pending
                    },
                    diagnostic_code: None,
                }
            })
            .collect::<Vec<_>>();
        resources.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
        let installation_ids = installations
            .iter()
            .filter(|installation| installation.source_id == source_id)
            .map(|installation| installation.id.clone())
            .collect::<Vec<_>>();
        let affected_project_count = installations
            .iter()
            .filter(|installation| installation.source_id == source_id)
            .map(|installation| &installation.canonical_project_path)
            .collect::<BTreeSet<_>>()
            .len();
        let affected_agent_count = installations
            .iter()
            .filter(|installation| installation.source_id == source_id)
            .map(|installation| (&installation.canonical_project_path, &installation.agent_id))
            .collect::<BTreeSet<_>>()
            .len();
        let plan_id = PlanId::from(format!("source-removal-plan:{}", uuid::Uuid::new_v4()));
        let risk_fingerprint = RiskFingerprint::from(opaque_contract_id(
            "source-removal-risk",
            &[
                source_id,
                &catalog.revision.to_string(),
                &resources
                    .iter()
                    .map(|resource| resource.resource_id.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                &installation_ids
                    .iter()
                    .map(|installation| installation.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ],
        ));
        let view = SourceRemovalPlanView {
            plan_id: plan_id.clone(),
            source_id: source_id.to_owned(),
            source_name: source.display_name.clone(),
            expected_catalog_revision: catalog.revision,
            affected_project_count,
            affected_agent_count,
            resources,
            risk_fingerprint,
            expires_at: Utc::now() + Duration::minutes(REMOVAL_PLAN_TTL_MINUTES),
        };
        self.plans
            .lock()
            .map_err(|_| lifecycle_error("Source removal plan store is unavailable"))?
            .insert(
                plan_id,
                StoredSourceRemovalPlan {
                    view: view.clone(),
                    installation_ids,
                },
            );
        Ok(view)
    }

    pub fn apply(
        &self,
        plan_id: &PlanId,
        risk_fingerprint: &RiskFingerprint,
        confirmed: bool,
        resource_plans: &ResourceRemovalPlanStore,
        source_plans: &SkillCatalogPlanStore,
        report: &dyn Fn(SourceRemovalProgress),
    ) -> Result<SourceRemovalReport, AgentError> {
        if !confirmed {
            return Err(lifecycle_error("Source removal confirmation is required"));
        }
        let stored = self
            .plans
            .lock()
            .map_err(|_| lifecycle_error("Source removal plan store is unavailable"))?
            .remove(plan_id)
            .ok_or_else(|| lifecycle_error("Unknown source removal plan"))?;
        if stored.view.expires_at <= Utc::now() || &stored.view.risk_fingerprint != risk_fingerprint
        {
            return Err(lifecycle_error("Source removal plan is stale"));
        }
        let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
        let mut current_resource_ids = catalog
            .resources
            .values()
            .filter(|resource| resource.source_id == stored.view.source_id)
            .map(|resource| resource.id.clone())
            .collect::<Vec<_>>();
        current_resource_ids.sort();
        let planned_resource_ids = stored
            .view
            .resources
            .iter()
            .map(|resource| resource.resource_id.clone())
            .collect::<Vec<_>>();
        let current_installation_ids = list_resource_installations_for_lifecycle()
            .map_err(lifecycle_error)?
            .into_iter()
            .filter(|installation| installation.source_id == stored.view.source_id)
            .map(|installation| installation.id)
            .collect::<Vec<_>>();
        if catalog.revision != stored.view.expected_catalog_revision
            || current_resource_ids != planned_resource_ids
            || current_installation_ids != stored.installation_ids
        {
            return Err(lifecycle_error(
                "Source removal impact changed after preview",
            ));
        }

        let operation_id = format!("source-removal:{}", uuid::Uuid::new_v4());
        let total = stored.view.resources.len();
        let mut completed = 0;
        let mut sequence = 0;
        let mut resources = stored.view.resources.clone();
        emit_source(
            report,
            &operation_id,
            &mut sequence,
            SourceRemovalPhase::Uninstalling,
            completed,
            total,
            None,
        );
        for resource in &mut resources {
            if resource.state == ResourceRemovalItemState::Succeeded {
                completed += 1;
                continue;
            }
            resource.state = ResourceRemovalItemState::Running;
            emit_source(
                report,
                &operation_id,
                &mut sequence,
                SourceRemovalPhase::Uninstalling,
                completed,
                total,
                Some(resource.clone()),
            );
            let outcome = resource_plans
                .preview(&resource.resource_id)
                .and_then(|plan| {
                    resource_plans.apply(&plan.plan_id, &plan.risk_fingerprint, true, &|_| {})
                });
            match outcome {
                Ok(removal) if removal.phase == ResourceRemovalPhase::Complete => {
                    resource.state = ResourceRemovalItemState::Succeeded;
                    completed += 1;
                }
                Ok(_) => {
                    resource.state = ResourceRemovalItemState::Failed;
                    resource.diagnostic_code = Some("resource_removal_partial_failure".into());
                }
                Err(error) => {
                    resource.state = ResourceRemovalItemState::Failed;
                    resource.diagnostic_code =
                        Some(format!("{:?}", error.code).to_ascii_lowercase());
                }
            }
            emit_source(
                report,
                &operation_id,
                &mut sequence,
                SourceRemovalPhase::Uninstalling,
                completed,
                total,
                Some(resource.clone()),
            );
        }
        if resources
            .iter()
            .any(|resource| resource.state == ResourceRemovalItemState::Failed)
        {
            emit_source(
                report,
                &operation_id,
                &mut sequence,
                SourceRemovalPhase::PartialFailure,
                completed,
                total,
                None,
            );
            return Ok(SourceRemovalReport {
                operation_id,
                source_id: stored.view.source_id,
                phase: SourceRemovalPhase::PartialFailure,
                completed,
                total,
                resources,
            });
        }

        emit_source(
            report,
            &operation_id,
            &mut sequence,
            SourceRemovalPhase::RemovingSource,
            completed,
            total,
            None,
        );
        let source_plan = source_plans
            .preview_remove(&stored.view.source_id)
            .map_err(lifecycle_error)?;
        apply_skill_catalog_plan(
            source_plans,
            &SkillCatalogPlanClaim {
                plan_id: source_plan.id,
                risk_fingerprint: source_plan.risk_fingerprint,
                confirmed: true,
            },
        )
        .map_err(lifecycle_error)?;
        emit_source(
            report,
            &operation_id,
            &mut sequence,
            SourceRemovalPhase::Complete,
            completed,
            total,
            None,
        );
        Ok(SourceRemovalReport {
            operation_id,
            source_id: stored.view.source_id,
            phase: SourceRemovalPhase::Complete,
            completed,
            total,
            resources,
        })
    }
}

impl ResourceRemovalPlanStore {
    pub fn preview(&self, resource_id: &str) -> Result<ResourceRemovalPlanView, AgentError> {
        let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
        let resource = catalog
            .resources
            .get(resource_id)
            .filter(|resource| resource.lifecycle == ResourceLifecycle::Managed)
            .ok_or_else(|| lifecycle_error("Managed resource was not found"))?;
        let installations = list_resource_installations_for_lifecycle()
            .map_err(lifecycle_error)?
            .into_iter()
            .filter(|installation| installation.resource_id == resource_id)
            .collect::<Vec<_>>();
        let views = installations
            .iter()
            .map(installation_view)
            .collect::<Vec<_>>();
        let affected_project_count = views
            .iter()
            .map(|view| &view.project_path)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let affected_agent_count = views
            .iter()
            .map(|view| (&view.project_path, &view.agent_id))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let plan_id = PlanId::from(uuid::Uuid::new_v4().to_string());
        let risk_fingerprint = RiskFingerprint::from(opaque_contract_id(
            "resource-removal-risk",
            &[
                resource_id,
                &catalog.revision.to_string(),
                &installations
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ],
        ));
        let view = ResourceRemovalPlanView {
            plan_id: plan_id.clone(),
            resource_id: resource_id.to_owned(),
            resource_name: resource.display_name.clone(),
            expected_catalog_revision: catalog.revision,
            affected_project_count,
            affected_agent_count,
            installations: views,
            risk_fingerprint,
            expires_at: Utc::now() + Duration::minutes(REMOVAL_PLAN_TTL_MINUTES),
        };
        self.plans
            .lock()
            .map_err(|_| lifecycle_error("Removal plan store is unavailable"))?
            .insert(
                plan_id,
                StoredRemovalPlan {
                    view: view.clone(),
                    installation_ids: installations.into_iter().map(|item| item.id).collect(),
                },
            );
        Ok(view)
    }

    pub fn apply(
        &self,
        plan_id: &PlanId,
        risk_fingerprint: &RiskFingerprint,
        confirmed: bool,
        report: &dyn Fn(ResourceRemovalProgress),
    ) -> Result<ResourceRemovalReport, AgentError> {
        if !confirmed {
            return Err(lifecycle_error("Resource removal confirmation is required"));
        }
        let stored = self
            .plans
            .lock()
            .map_err(|_| lifecycle_error("Removal plan store is unavailable"))?
            .remove(plan_id)
            .ok_or_else(|| lifecycle_error("Unknown resource removal plan"))?;
        if stored.view.expires_at <= Utc::now() || &stored.view.risk_fingerprint != risk_fingerprint
        {
            return Err(lifecycle_error("Resource removal plan is stale"));
        }
        self.apply_stored(stored, report, None)
    }

    fn apply_stored(
        &self,
        stored: StoredRemovalPlan,
        report: &dyn Fn(ResourceRemovalProgress),
        seed: Option<ResourceRemovalOperationSeed>,
    ) -> Result<ResourceRemovalReport, AgentError> {
        let state = super::execution_state::ExecutionState::open().map_err(lifecycle_error)?;
        let lifecycle_operation_id = format!("resource-removal-lease:{}", uuid::Uuid::new_v4());
        let _lifecycle_lease = TargetLockSet::acquire_for_ad_states(
            &[resource_lifecycle_lock_target(
                &state,
                &stored.view.resource_id,
            )],
            &lifecycle_operation_id,
            &state,
        )
        .map_err(lifecycle_error)?;
        let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
        let current_ids = list_resource_installations_for_lifecycle()
            .map_err(lifecycle_error)?
            .into_iter()
            .filter(|installation| installation.resource_id == stored.view.resource_id)
            .map(|installation| installation.id)
            .collect::<Vec<_>>();
        if catalog.revision != stored.view.expected_catalog_revision
            || current_ids != stored.installation_ids
        {
            return Err(lifecycle_error(
                "Resource removal impact changed after preview",
            ));
        }
        {
            let mut active = self
                .active_resources
                .lock()
                .map_err(|_| lifecycle_error("Removal operation registry is unavailable"))?;
            if !active.insert(stored.view.resource_id.clone()) {
                return Err(lifecycle_error(
                    "A removal operation is already running for this resource",
                ));
            }
        }
        let resource_id = stored.view.resource_id.clone();
        let result = self.apply_inner(stored, report, &state, seed);
        if let Ok(mut active) = self.active_resources.lock() {
            active.remove(&resource_id);
        }
        result
    }

    fn apply_inner(
        &self,
        stored: StoredRemovalPlan,
        report: &dyn Fn(ResourceRemovalProgress),
        state: &super::execution_state::ExecutionState,
        seed: Option<ResourceRemovalOperationSeed>,
    ) -> Result<ResourceRemovalReport, AgentError> {
        let (operation_id, started_at, mut views) = seed.map_or_else(
            || {
                (
                    format!("resource-removal:{}", uuid::Uuid::new_v4()),
                    Utc::now(),
                    stored.view.installations.clone(),
                )
            },
            |seed| (seed.operation_id, seed.started_at, seed.installations),
        );
        let total = views.len();
        let mut sequence = 0;
        let mut completed = views
            .iter()
            .filter(|view| view.state == ResourceRemovalItemState::Succeeded)
            .count();
        persist_operation(&ResourceRemovalOperationSnapshot {
            schema_version: 1,
            operation_id: operation_id.clone(),
            resource_id: stored.view.resource_id.clone(),
            started_at,
            updated_at: started_at,
            phase: ResourceRemovalPhase::Uninstalling,
            completed,
            total,
            installations: views.clone(),
        })?;
        emit(
            report,
            &operation_id,
            &mut sequence,
            ResourceRemovalPhase::Uninstalling,
            completed,
            total,
            None,
        );
        for index in 0..views.len() {
            if views[index].state == ResourceRemovalItemState::Succeeded {
                continue;
            }
            views[index].state = ResourceRemovalItemState::Running;
            views[index].diagnostic_code = None;
            emit(
                report,
                &operation_id,
                &mut sequence,
                ResourceRemovalPhase::Uninstalling,
                completed,
                total,
                Some(views[index].clone()),
            );
            match uninstall(&views[index]) {
                Ok(()) => {
                    completed += 1;
                    views[index].state = ResourceRemovalItemState::Succeeded;
                }
                Err(error) => {
                    views[index].state = ResourceRemovalItemState::Failed;
                    views[index].diagnostic_code =
                        Some(format!("{:?}", error.code).to_ascii_lowercase());
                }
            }
            emit(
                report,
                &operation_id,
                &mut sequence,
                ResourceRemovalPhase::Uninstalling,
                completed,
                total,
                Some(views[index].clone()),
            );
            persist_operation(&ResourceRemovalOperationSnapshot {
                schema_version: 1,
                operation_id: operation_id.clone(),
                resource_id: stored.view.resource_id.clone(),
                started_at,
                updated_at: Utc::now(),
                phase: ResourceRemovalPhase::Uninstalling,
                completed,
                total,
                installations: views.clone(),
            })?;
        }
        let remaining = list_resource_installations_for_lifecycle()
            .map_err(lifecycle_error)?
            .into_iter()
            .any(|installation| installation.resource_id == stored.view.resource_id);
        let phase = if remaining {
            ResourceRemovalPhase::PartialFailure
        } else {
            emit(
                report,
                &operation_id,
                &mut sequence,
                ResourceRemovalPhase::Suppressing,
                completed,
                total,
                None,
            );
            set_resource_lifecycle_under_lease(
                state,
                &stored.view.resource_id,
                ResourceLifecycle::Suppressed,
            )
            .map_err(lifecycle_error)?;
            ResourceRemovalPhase::Complete
        };
        let operation = ResourceRemovalOperationSnapshot {
            schema_version: 1,
            operation_id: operation_id.clone(),
            resource_id: stored.view.resource_id.clone(),
            started_at,
            updated_at: Utc::now(),
            phase,
            completed,
            total,
            installations: views.clone(),
        };
        persist_operation(&operation)?;
        emit(
            report,
            &operation_id,
            &mut sequence,
            phase,
            completed,
            total,
            None,
        );
        Ok(ResourceRemovalReport {
            operation_id,
            resource_id: stored.view.resource_id,
            phase,
            completed,
            total,
            installations: views,
        })
    }

    pub fn retry(
        &self,
        operation_id: &str,
        report: &dyn Fn(ResourceRemovalProgress),
    ) -> Result<ResourceRemovalReport, AgentError> {
        let operation = load_operation(operation_id)?;
        if operation.phase == ResourceRemovalPhase::Complete {
            return Err(lifecycle_error("Resource removal is already complete"));
        }
        let preview = self.preview(&operation.resource_id)?;
        let stored = self
            .plans
            .lock()
            .map_err(|_| lifecycle_error("Removal plan store is unavailable"))?
            .remove(&preview.plan_id)
            .ok_or_else(|| lifecycle_error("Unknown resource removal plan"))?;
        let mut installations = operation.installations;
        for current in &stored.view.installations {
            if !installations
                .iter()
                .any(|item| item.installation_id == current.installation_id)
            {
                installations.push(current.clone());
            }
        }
        self.apply_stored(
            stored,
            report,
            Some(ResourceRemovalOperationSeed {
                operation_id: operation.operation_id,
                started_at: operation.started_at,
                installations,
            }),
        )
    }
}

pub fn list_resource_removal_operations(
) -> Result<Vec<ResourceRemovalOperationSnapshot>, AgentError> {
    let state = super::execution_state::ExecutionState::open().map_err(lifecycle_error)?;
    let active_installations =
        list_resource_installations_for_lifecycle().map_err(lifecycle_error)?;
    let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
    let mut operations = Vec::new();
    for name in state
        .resource_removal_operations()
        .entry_names()
        .map_err(lifecycle_error)?
    {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let bytes = state
            .resource_removal_operations()
            .read(name)
            .map_err(lifecycle_error)?;
        let mut operation: ResourceRemovalOperationSnapshot =
            serde_json::from_slice(&bytes).map_err(lifecycle_error)?;
        if operation.schema_version != 1
            || operation.operation_id.is_empty()
            || operation.resource_id.is_empty()
        {
            return Err(lifecycle_error("Resource removal operation is corrupt"));
        }
        let active_ids = active_installations
            .iter()
            .filter(|installation| installation.resource_id == operation.resource_id)
            .map(|installation| &installation.id)
            .collect::<BTreeSet<_>>();
        for item in &mut operation.installations {
            if !active_ids.contains(&item.installation_id) {
                item.state = ResourceRemovalItemState::Succeeded;
                item.diagnostic_code = None;
            } else if item.state == ResourceRemovalItemState::Running {
                item.state = ResourceRemovalItemState::Failed;
                item.diagnostic_code = Some("interrupted".into());
            }
        }
        operation.completed = operation
            .installations
            .iter()
            .filter(|item| item.state == ResourceRemovalItemState::Succeeded)
            .count();
        if active_ids.is_empty()
            && catalog
                .resources
                .get(&operation.resource_id)
                .is_some_and(|resource| resource.lifecycle == ResourceLifecycle::Suppressed)
        {
            operation.phase = ResourceRemovalPhase::Complete;
        } else if operation.phase != ResourceRemovalPhase::Complete {
            operation.phase = ResourceRemovalPhase::PartialFailure;
        }
        operations.push(operation);
    }
    operations.sort_by_key(|operation| operation.updated_at);
    Ok(operations)
}

fn load_operation(operation_id: &str) -> Result<ResourceRemovalOperationSnapshot, AgentError> {
    list_resource_removal_operations()?
        .into_iter()
        .find(|operation| operation.operation_id == operation_id)
        .ok_or_else(|| lifecycle_error("Resource removal operation was not found"))
}

fn persist_operation(operation: &ResourceRemovalOperationSnapshot) -> Result<(), AgentError> {
    let state = super::execution_state::ExecutionState::open().map_err(lifecycle_error)?;
    let name = format!("{}.json", operation.operation_id.replace(':', "_"));
    let bytes = serde_json::to_vec_pretty(operation).map_err(lifecycle_error)?;
    state
        .resource_removal_operations()
        .write_atomic(&name, &bytes)
        .map_err(lifecycle_error)
}

pub fn readd_catalog_resource(
    resource_id: &str,
) -> Result<super::ResourceCatalogSnapshot, AgentError> {
    let catalog = load_resource_catalog_snapshot().map_err(lifecycle_error)?;
    let resource = catalog
        .resources
        .get(resource_id)
        .filter(|resource| resource.lifecycle == ResourceLifecycle::Suppressed && resource.present)
        .ok_or_else(|| lifecycle_error("Suppressed resource is not available in its source"))?;
    let source = catalog
        .sources
        .get(&resource.source_id)
        .and_then(|source| source.binding.as_ref())
        .ok_or_else(|| lifecycle_error("Resource source binding is unavailable"))?;
    let physical_root = std::path::Path::new(&source.physical_root);
    let matches = super::resource_scanner::scan_catalog_resources(physical_root)
        .map_err(lifecycle_error)?
        .into_iter()
        .filter(|candidate| {
            candidate.kind == resource.kind
                && candidate.install_id == resource.install_id
                && candidate.subpath == resource.subpath
        })
        .count();
    if matches != 1 {
        return Err(lifecycle_error(
            "Resource must appear exactly once in the live source before re-adding",
        ));
    }
    set_resource_lifecycle(resource_id, ResourceLifecycle::Managed).map_err(lifecycle_error)
}

fn uninstall(view: &ResourceRemovalInstallationView) -> Result<(), AgentError> {
    let record = list_resource_installations_for_lifecycle()
        .map_err(lifecycle_error)?
        .into_iter()
        .find(|record| record.id == view.installation_id)
        .ok_or_else(|| lifecycle_error("Installation disappeared during removal"))?;
    uninstall_record(&record)
}

fn uninstall_record(record: &ResourceInstallationRecord) -> Result<(), AgentError> {
    let state = super::execution_state::ExecutionState::open().map_err(lifecycle_error)?;
    for ownership_id in &record.ownership_record_ids {
        let ownership = super::load_ownership_record_by_id(&state, ownership_id)?
            .ok_or_else(|| lifecycle_error("Managed installation ownership is unavailable"))?;
        validate_ownership_target(&ownership)?;
    }
    let context = AgentContext {
        installation_id: record.effective_installation_id.clone(),
        project_path: Some(record.canonical_project_path.clone()),
    };
    let resource = ResourceRef {
        installation_id: record.effective_installation_id.clone(),
        project_path: Some(record.canonical_project_path.clone()),
        kind: record.resource_kind,
        scope: ResourceScope::Project,
        logical_id: format!("{}/{}", record.source_id, record.install_id),
    };
    let registry = builtin_registry();
    let adapter = registry.adapter_for_context(&context)?;
    let plan = match record.resource_kind {
        ResourceKind::Skills => adapter
            .skills()
            .ok_or_else(|| lifecycle_error("Agent Skill adapter is unavailable"))?
            .plan_remove(&context, &resource)?,
        ResourceKind::Plugins => adapter
            .plugins()
            .ok_or_else(|| lifecycle_error("Agent Plugin adapter is unavailable"))?
            .plan_remove(&context, &resource)?,
        _ => return Err(lifecycle_error("Unsupported managed resource kind")),
    };
    let plans = PlanStore::default();
    let view: MutationPlanView = plans.insert(plan)?;
    ExecutionEngine.apply_bound(&view.id, &view.context, &view.risk_fingerprint, &plans)?;
    Ok(())
}

fn installation_view(record: &ResourceInstallationRecord) -> ResourceRemovalInstallationView {
    ResourceRemovalInstallationView {
        installation_id: record.id.clone(),
        workspace_key: record.workspace_key.clone(),
        agent_id: record.agent_id.clone(),
        project_path: record.canonical_project_path.clone(),
        state: ResourceRemovalItemState::Pending,
        diagnostic_code: None,
    }
}

fn emit(
    report: &dyn Fn(ResourceRemovalProgress),
    operation_id: &str,
    sequence: &mut u64,
    phase: ResourceRemovalPhase,
    completed: usize,
    total: usize,
    item: Option<ResourceRemovalInstallationView>,
) {
    *sequence += 1;
    report(ResourceRemovalProgress {
        operation_id: operation_id.to_owned(),
        sequence: *sequence,
        phase,
        completed,
        total,
        item,
    });
}

fn emit_source(
    report: &dyn Fn(SourceRemovalProgress),
    operation_id: &str,
    sequence: &mut u64,
    phase: SourceRemovalPhase,
    completed: usize,
    total: usize,
    item: Option<SourceRemovalResourceView>,
) {
    *sequence += 1;
    report(SourceRemovalProgress {
        operation_id: operation_id.to_owned(),
        sequence: *sequence,
        phase,
        completed,
        total,
        item,
    });
}

fn lifecycle_error(error: impl std::fmt::Display) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: error.to_string(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "resource_lifecycle"})),
    }
}

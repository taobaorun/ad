use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::execution_state::ExecutionState;
use super::skill_catalog::{
    acquisition_source, load_skill_catalog_state, new_source_id, validate_request,
    SkillCatalogError,
};
use super::{
    inspect_local_skill_source_binding, stage_git_skill_source_binding, ContentDigest, PlanId,
    ReceiptId, ResourceOwnershipRecord, ResourceRef, RiskFingerprint, SkillArtifactError,
    SkillArtifactRef, SkillCatalogSnapshot, SkillSourceBinding, SkillSourceRequest,
    SkillSourceType, StagedGitSkillSourceBinding, WorkspaceKey,
};
use crate::models::SkillSource;

const PLAN_SCHEMA_VERSION: u32 = 1;
const PLAN_TTL_MINUTES: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogAction {
    Add,
    Update,
    Remove,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCatalogPlanApplicability {
    #[default]
    Applicable,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogBlockingIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogPlanView {
    pub schema_version: u32,
    pub id: PlanId,
    pub action: SkillCatalogAction,
    pub expected_catalog_revision: ContentDigest,
    pub source_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<SkillArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<SkillSourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_artifact: Option<SkillArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_binding: Option<SkillSourceBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of: Option<ReceiptId>,
    pub applicability: SkillCatalogPlanApplicability,
    #[serde(default)]
    pub blocking_issues: Vec<SkillCatalogBlockingIssue>,
    #[serde(default)]
    pub affected_resources: Vec<ResourceRef>,
    #[serde(default)]
    pub affected_workspaces: Vec<WorkspaceKey>,
    pub confirmation_required: bool,
    pub risk_fingerprint: RiskFingerprint,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogPlanClaim {
    pub plan_id: PlanId,
    pub risk_fingerprint: RiskFingerprint,
    pub confirmed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillCatalogPlanError {
    #[error(transparent)]
    Catalog(#[from] SkillCatalogError),
    #[error(transparent)]
    Artifact(#[from] SkillArtifactError),
    #[error("unknown or already consumed Skill catalog plan")]
    InvalidPlan,
    #[error("Skill catalog plan expired")]
    Expired,
    #[error("Skill catalog plan confirmation is required")]
    ConfirmationRequired,
    #[error("Skill catalog plan risk fingerprint changed")]
    RiskChanged,
    #[error("Skill catalog plan is blocked by current references")]
    Blocked,
    #[error("Skill catalog plan store is unavailable")]
    StoreUnavailable,
}

pub(crate) struct ClaimedSkillCatalogPlan {
    pub(crate) view: SkillCatalogPlanView,
    pub(crate) request: Option<SkillSourceRequest>,
    pub(crate) staged_git: Option<StagedGitSkillSourceBinding>,
    pub(crate) rollback_of: Option<ReceiptId>,
}

#[derive(Clone)]
pub(crate) struct SkillCatalogPlanBinding {
    pub(crate) view: SkillCatalogPlanView,
    pub(crate) source: Option<SkillSource>,
}

struct StoredSkillCatalogPlan {
    view: SkillCatalogPlanView,
    request: Option<SkillSourceRequest>,
    source: Option<SkillSource>,
    staged_git: Option<StagedGitSkillSourceBinding>,
    rollback_of: Option<ReceiptId>,
}

#[derive(Default)]
pub struct SkillCatalogPlanStore {
    plans: Mutex<HashMap<PlanId, StoredSkillCatalogPlan>>,
}

impl SkillCatalogPlanStore {
    pub fn cancel(&self, plan_id: &PlanId) -> Result<bool, SkillCatalogPlanError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| SkillCatalogPlanError::StoreUnavailable)?;
        Ok(plans.remove(plan_id).is_some())
    }

    pub fn preview_add(
        &self,
        request: SkillSourceRequest,
    ) -> Result<SkillCatalogPlanView, SkillCatalogPlanError> {
        validate_request(&request)?;
        let state = load_skill_catalog_state()?;
        let source_id = new_source_id();
        let source = acquisition_source(source_id.clone(), &request)?;
        let (artifact, binding, staged_git) = inspect_source(&source)?;
        let now = Utc::now();
        let mut candidate = state.document.clone();
        if let Some(binding) = &binding {
            candidate.add_binding(source_id.clone(), &request, binding.clone(), now)?;
        } else {
            candidate.add(
                source_id.clone(),
                &request,
                artifact.clone().ok_or(SkillCatalogPlanError::InvalidPlan)?,
                now,
            )?;
        }
        let view = plan_view(
            SkillCatalogAction::Add,
            &state.snapshot(),
            source_id,
            request.display_name.trim().to_owned(),
            SkillCatalogPlanPayload {
                artifact,
                binding,
                current_artifact: None,
                current_binding: None,
                rollback_of: None,
            },
            ReferenceImpact::default(),
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: Some(request),
            source: Some(source),
            staged_git,
            rollback_of: None,
        })?;
        Ok(view)
    }

    pub fn preview_update(
        &self,
        source_id: &str,
    ) -> Result<SkillCatalogPlanView, SkillCatalogPlanError> {
        let state = load_skill_catalog_state()?;
        let entry = state
            .document
            .entry(source_id)
            .cloned()
            .ok_or_else(|| SkillCatalogError::NotFound(source_id.to_owned()))?;
        let source = entry.acquisition_source();
        let (artifact, binding, staged_git) = inspect_source(&source)?;
        let now = Utc::now();
        let impact = reference_impact(&entry.source_id, binding.as_ref(), false)?;
        let view = plan_view(
            SkillCatalogAction::Update,
            &state.snapshot(),
            entry.source_id,
            entry.display_name,
            SkillCatalogPlanPayload {
                artifact,
                binding,
                current_artifact: entry.current_artifact,
                current_binding: entry.current_binding,
                rollback_of: None,
            },
            impact,
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: None,
            source: Some(source),
            staged_git,
            rollback_of: None,
        })?;
        Ok(view)
    }

    pub fn preview_remove(
        &self,
        source_id: &str,
    ) -> Result<SkillCatalogPlanView, SkillCatalogPlanError> {
        let state = load_skill_catalog_state()?;
        let entry = state
            .document
            .entry(source_id)
            .cloned()
            .ok_or_else(|| SkillCatalogError::NotFound(source_id.to_owned()))?;
        let now = Utc::now();
        let impact = reference_impact(&entry.source_id, None, true)?;
        let view = plan_view(
            SkillCatalogAction::Remove,
            &state.snapshot(),
            entry.source_id,
            entry.display_name,
            SkillCatalogPlanPayload {
                artifact: None,
                binding: None,
                current_artifact: entry.current_artifact,
                current_binding: entry.current_binding,
                rollback_of: None,
            },
            impact,
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: None,
            source: None,
            staged_git: None,
            rollback_of: None,
        })?;
        Ok(view)
    }

    pub(crate) fn preview_rollback(
        &self,
        receipt_id: ReceiptId,
        source_id: &str,
        expected_current: &SkillSourceBinding,
        target: SkillSourceBinding,
    ) -> Result<SkillCatalogPlanView, SkillCatalogPlanError> {
        let state = load_skill_catalog_state()?;
        let entry = state
            .document
            .entry(source_id)
            .cloned()
            .ok_or_else(|| SkillCatalogError::NotFound(source_id.to_owned()))?;
        if entry.current_binding.as_ref() != Some(expected_current) {
            return Err(SkillCatalogPlanError::InvalidPlan);
        }
        let now = Utc::now();
        let impact = reference_impact(source_id, Some(&target), false)?;
        let view = plan_view(
            SkillCatalogAction::Update,
            &state.snapshot(),
            entry.source_id,
            entry.display_name,
            SkillCatalogPlanPayload {
                artifact: None,
                binding: Some(target),
                current_artifact: entry.current_artifact,
                current_binding: entry.current_binding,
                rollback_of: Some(receipt_id.clone()),
            },
            impact,
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: None,
            source: None,
            staged_git: None,
            rollback_of: Some(receipt_id),
        })?;
        Ok(view)
    }

    pub(crate) fn binding(
        &self,
        claim: &SkillCatalogPlanClaim,
        now: DateTime<Utc>,
    ) -> Result<SkillCatalogPlanBinding, SkillCatalogPlanError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| SkillCatalogPlanError::StoreUnavailable)?;
        prune_expired(&mut plans, now);
        let stored = plans
            .get(&claim.plan_id)
            .ok_or(SkillCatalogPlanError::InvalidPlan)?;
        validate_claim(&stored.view, claim, now)?;
        Ok(SkillCatalogPlanBinding {
            view: stored.view.clone(),
            source: stored.source.clone(),
        })
    }

    pub(crate) fn claim(
        &self,
        claim: &SkillCatalogPlanClaim,
        now: DateTime<Utc>,
    ) -> Result<ClaimedSkillCatalogPlan, SkillCatalogPlanError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| SkillCatalogPlanError::StoreUnavailable)?;
        prune_expired(&mut plans, now);
        let stored = plans
            .get(&claim.plan_id)
            .ok_or(SkillCatalogPlanError::InvalidPlan)?;
        validate_claim(&stored.view, claim, now)?;
        let stored = plans
            .remove(&claim.plan_id)
            .ok_or(SkillCatalogPlanError::InvalidPlan)?;
        Ok(ClaimedSkillCatalogPlan {
            view: stored.view,
            request: stored.request,
            staged_git: stored.staged_git,
            rollback_of: stored.rollback_of,
        })
    }

    fn insert(&self, plan: StoredSkillCatalogPlan) -> Result<(), SkillCatalogPlanError> {
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| SkillCatalogPlanError::StoreUnavailable)?;
        prune_expired(&mut plans, Utc::now());
        plans.insert(plan.view.id.clone(), plan);
        Ok(())
    }
}

fn validate_claim(
    view: &SkillCatalogPlanView,
    claim: &SkillCatalogPlanClaim,
    now: DateTime<Utc>,
) -> Result<(), SkillCatalogPlanError> {
    if view.expires_at <= now {
        return Err(SkillCatalogPlanError::Expired);
    }
    if view.applicability == SkillCatalogPlanApplicability::Blocked {
        return Err(SkillCatalogPlanError::Blocked);
    }
    if view.risk_fingerprint != claim.risk_fingerprint {
        return Err(SkillCatalogPlanError::RiskChanged);
    }
    if view.confirmation_required && !claim.confirmed {
        return Err(SkillCatalogPlanError::ConfirmationRequired);
    }
    Ok(())
}

fn prune_expired(plans: &mut HashMap<PlanId, StoredSkillCatalogPlan>, now: DateTime<Utc>) {
    plans.retain(|_, plan| plan.view.expires_at > now);
}

fn plan_view(
    action: SkillCatalogAction,
    catalog: &SkillCatalogSnapshot,
    source_id: String,
    display_name: String,
    payload: SkillCatalogPlanPayload,
    impact: ReferenceImpact,
    now: DateTime<Utc>,
) -> SkillCatalogPlanView {
    let SkillCatalogPlanPayload {
        artifact,
        binding,
        current_artifact,
        current_binding,
        rollback_of,
    } = payload;
    let id = PlanId::from(format!("skill-catalog-plan:{}", uuid::Uuid::new_v4()));
    let risk = serde_json::json!({
        "action": action,
        "catalogRevision": catalog.revision,
        "sourceId": source_id,
        "artifact": artifact.as_ref().map(|value| &value.artifact_id),
        "binding": binding.as_ref().map(|value| (&value.binding_id, &value.stable_root, &value.tree_digest)),
        "currentArtifact": current_artifact.as_ref().map(|value| &value.artifact_id),
        "currentBinding": current_binding.as_ref().map(|value| (&value.binding_id, &value.stable_root, &value.tree_digest)),
        "rollbackOf": rollback_of,
        "activationImpact": artifact.as_ref().map(|value| &value.activation_impact.digest)
            .or_else(|| binding.as_ref().map(|value| &value.activation_impact.digest)),
        "applicability": impact.applicability,
        "blockingIssues": impact.blocking_issues,
        "affectedResources": impact.affected_resources,
        "affectedWorkspaces": impact.affected_workspaces,
    });
    let risk_bytes = serde_json::to_vec(&risk).expect("risk facts are serializable");
    SkillCatalogPlanView {
        schema_version: PLAN_SCHEMA_VERSION,
        id,
        action,
        expected_catalog_revision: catalog.revision.clone(),
        source_id,
        display_name,
        artifact,
        binding,
        current_artifact,
        current_binding,
        rollback_of,
        applicability: impact.applicability,
        blocking_issues: impact.blocking_issues,
        affected_resources: impact.affected_resources,
        affected_workspaces: impact.affected_workspaces,
        confirmation_required: true,
        risk_fingerprint: RiskFingerprint::from(format!(
            "risk:{}",
            ContentDigest::sha256(&risk_bytes)
        )),
        expires_at: now + Duration::minutes(PLAN_TTL_MINUTES),
    }
}

struct SkillCatalogPlanPayload {
    artifact: Option<SkillArtifactRef>,
    binding: Option<SkillSourceBinding>,
    current_artifact: Option<SkillArtifactRef>,
    current_binding: Option<SkillSourceBinding>,
    rollback_of: Option<ReceiptId>,
}

#[derive(Default)]
struct ReferenceImpact {
    applicability: SkillCatalogPlanApplicability,
    blocking_issues: Vec<SkillCatalogBlockingIssue>,
    affected_resources: Vec<ResourceRef>,
    affected_workspaces: Vec<WorkspaceKey>,
}

fn reference_impact(
    source_id: &str,
    candidate: Option<&SkillSourceBinding>,
    removing: bool,
) -> Result<ReferenceImpact, SkillCatalogPlanError> {
    let state = ExecutionState::open().map_err(|error| {
        SkillCatalogError::Corrupt(format!("ownership index is unavailable: {error}"))
    })?;
    let mut impact = ReferenceImpact::default();
    let mut workspaces = BTreeSet::new();
    for name in state.ownership().entry_names().map_err(|error| {
        SkillCatalogError::Corrupt(format!("ownership index is unavailable: {error}"))
    })? {
        let Some(name) = name.to_str() else {
            block_unknown_ownership(&mut impact);
            continue;
        };
        let bytes = state.ownership().read(name).map_err(|error| {
            SkillCatalogError::Corrupt(format!("ownership record is unavailable: {error}"))
        })?;
        let record = match serde_json::from_slice::<ResourceOwnershipRecord>(&bytes) {
            Ok(record) => record,
            Err(_) => {
                block_unknown_ownership(&mut impact);
                continue;
            }
        };
        let references_source = record
            .source_binding
            .as_ref()
            .is_some_and(|binding| binding.source_id == source_id)
            || record
                .resource
                .logical_id
                .strip_prefix(source_id)
                .is_some_and(|suffix| suffix.starts_with('/'));
        if !references_source {
            continue;
        }
        workspaces.insert(record.workspace_key.clone());
        impact.affected_resources.push(record.resource.clone());
        if removing {
            impact.blocking_issues.push(SkillCatalogBlockingIssue {
                code: "source_has_installed_skills".into(),
                message: "Remove the installed Skills before removing this source".into(),
                resource: Some(record.resource.clone()),
            });
        } else if candidate.is_some_and(|binding| {
            record.source_binding.as_ref().is_some_and(|owned| {
                !binding
                    .skills
                    .iter()
                    .any(|item| item.subpath == owned.skill_subpath)
            })
        }) {
            impact.blocking_issues.push(SkillCatalogBlockingIssue {
                code: "source_update_breaks_installed_skill".into(),
                message: "The candidate Git revision removes an installed Skill".into(),
                resource: Some(record.resource.clone()),
            });
        }
    }
    impact.affected_workspaces = workspaces.into_iter().collect();
    impact.affected_resources.sort_by(|left, right| {
        left.logical_id
            .cmp(&right.logical_id)
            .then_with(|| left.project_path.cmp(&right.project_path))
    });
    impact.blocking_issues.sort_by(|left, right| {
        left.code.cmp(&right.code).then_with(|| {
            left.resource
                .as_ref()
                .map(|resource| &resource.logical_id)
                .cmp(&right.resource.as_ref().map(|resource| &resource.logical_id))
        })
    });
    if !impact.blocking_issues.is_empty() {
        impact.applicability = SkillCatalogPlanApplicability::Blocked;
    }
    Ok(impact)
}

pub(crate) fn skill_catalog_plan_references_are_current(
    view: &SkillCatalogPlanView,
) -> Result<bool, SkillCatalogPlanError> {
    if view.action == SkillCatalogAction::Add {
        return Ok(view.blocking_issues.is_empty()
            && view.affected_resources.is_empty()
            && view.affected_workspaces.is_empty());
    }
    let impact = reference_impact(
        &view.source_id,
        view.binding.as_ref(),
        view.action == SkillCatalogAction::Remove,
    )?;
    Ok(impact.applicability == view.applicability
        && impact.blocking_issues == view.blocking_issues
        && impact.affected_resources == view.affected_resources
        && impact.affected_workspaces == view.affected_workspaces)
}

fn block_unknown_ownership(impact: &mut ReferenceImpact) {
    if impact
        .blocking_issues
        .iter()
        .all(|issue| issue.code != "ownership_index_unreadable")
    {
        impact.blocking_issues.push(SkillCatalogBlockingIssue {
            code: "ownership_index_unreadable".into(),
            message: "A malformed ownership record prevents safe source mutation".into(),
            resource: None,
        });
    }
}

type InspectedSkillSource = (
    Option<SkillArtifactRef>,
    Option<SkillSourceBinding>,
    Option<StagedGitSkillSourceBinding>,
);

fn inspect_source(source: &SkillSource) -> Result<InspectedSkillSource, SkillCatalogPlanError> {
    match source.source_type {
        SkillSourceType::Local => Ok((
            None,
            Some(inspect_local_skill_source_binding(source)?),
            None,
        )),
        SkillSourceType::Git => {
            let staged = stage_git_skill_source_binding(source)?;
            Ok((None, Some(staged.binding().clone()), Some(staged)))
        }
    }
}

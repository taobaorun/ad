use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::skill_catalog::{
    acquisition_source, load_skill_catalog_state, new_source_id, validate_request,
    SkillCatalogError,
};
use super::{
    stage_skill_source, ContentDigest, PlanId, RiskFingerprint, SkillArtifactError,
    SkillArtifactRef, SkillCatalogSnapshot, SkillSourceRequest, StagedSkillArtifact,
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
    pub current_artifact: Option<SkillArtifactRef>,
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
    #[error("Skill catalog plan store is unavailable")]
    StoreUnavailable,
}

pub(crate) struct ClaimedSkillCatalogPlan {
    pub(crate) view: SkillCatalogPlanView,
    pub(crate) request: Option<SkillSourceRequest>,
    pub(crate) staged: Option<StagedSkillArtifact>,
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
    staged: Option<StagedSkillArtifact>,
}

#[derive(Default)]
pub struct SkillCatalogPlanStore {
    plans: Mutex<HashMap<PlanId, StoredSkillCatalogPlan>>,
}

impl SkillCatalogPlanStore {
    pub fn preview_add(
        &self,
        request: SkillSourceRequest,
    ) -> Result<SkillCatalogPlanView, SkillCatalogPlanError> {
        validate_request(&request)?;
        let state = load_skill_catalog_state()?;
        let source_id = new_source_id();
        let source = acquisition_source(source_id.clone(), &request)?;
        let staged = stage_skill_source(&source)?;
        let now = Utc::now();
        let mut candidate = state.document.clone();
        candidate.add(source_id.clone(), &request, staged.reference().clone(), now)?;
        let view = plan_view(
            SkillCatalogAction::Add,
            &state.snapshot(),
            source_id,
            request.display_name.trim().to_owned(),
            Some(staged.reference().clone()),
            None,
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: Some(request),
            source: Some(source),
            staged: Some(staged),
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
        let staged = stage_skill_source(&source)?;
        let now = Utc::now();
        let view = plan_view(
            SkillCatalogAction::Update,
            &state.snapshot(),
            entry.source_id,
            entry.display_name,
            Some(staged.reference().clone()),
            Some(entry.current_artifact),
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: None,
            source: Some(source),
            staged: Some(staged),
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
        let view = plan_view(
            SkillCatalogAction::Remove,
            &state.snapshot(),
            entry.source_id,
            entry.display_name,
            None,
            Some(entry.current_artifact),
            now,
        );
        self.insert(StoredSkillCatalogPlan {
            view: view.clone(),
            request: None,
            source: None,
            staged: None,
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
            staged: stored.staged,
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
    artifact: Option<SkillArtifactRef>,
    current_artifact: Option<SkillArtifactRef>,
    now: DateTime<Utc>,
) -> SkillCatalogPlanView {
    let id = PlanId::from(format!("skill-catalog-plan:{}", uuid::Uuid::new_v4()));
    let risk = serde_json::json!({
        "action": action,
        "catalogRevision": catalog.revision,
        "sourceId": source_id,
        "artifact": artifact.as_ref().map(|value| &value.artifact_id),
        "currentArtifact": current_artifact.as_ref().map(|value| &value.artifact_id),
        "activationImpact": artifact.as_ref().map(|value| &value.activation_impact.digest),
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
        current_artifact,
        confirmation_required: true,
        risk_fingerprint: RiskFingerprint::from(format!(
            "risk:{}",
            ContentDigest::sha256(&risk_bytes)
        )),
        expires_at: now + Duration::minutes(PLAN_TTL_MINUTES),
    }
}

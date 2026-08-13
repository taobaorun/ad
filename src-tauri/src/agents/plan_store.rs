use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use super::execution_confinement::{capture_project_root_identity, ProjectRootIdentity};

use super::{
    AcknowledgementRequirement, AgentError, AgentErrorCode, ContentDigest, ConversionReport,
    MutationPlan, MutationPlanView, OperationKind, OwnershipRestore, PhysicalTargetId,
    PlanAcknowledgement, PlanAcknowledgementCode, PlanId, PlanRiskLevel, ReceiptId, ResourceRef,
    RiskFingerprint,
};

#[derive(Default)]
struct PlanState {
    active: HashMap<PlanId, StoredPlan>,
    consumed: HashMap<PlanId, DateTime<Utc>>,
}

struct StoredPlan {
    plan: MutationPlan,
    required_acknowledgements: Vec<AcknowledgementRequirement>,
    intent: PlanExecutionIntent,
    conversion_report: Option<Arc<ConversionReport>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlanExecutionIntent {
    pub(super) operation_kind: OperationKind,
    pub(super) parent_receipt_id: Option<ReceiptId>,
    pub(super) ownership_restores: BTreeMap<PhysicalTargetId, OwnershipRestore>,
    pub(super) project_root_identity: Option<ProjectRootIdentity>,
    pub(super) action_id: Option<String>,
}

impl PlanExecutionIntent {
    fn apply(plan: &MutationPlan) -> Result<Self, AgentError> {
        Ok(Self {
            operation_kind: OperationKind::Apply,
            parent_receipt_id: None,
            ownership_restores: BTreeMap::new(),
            project_root_identity: capture_project_root_identity(&plan.context)?,
            action_id: None,
        })
    }

    fn rollback(
        plan: &MutationPlan,
        parent_receipt_id: ReceiptId,
        ownership_restores: BTreeMap<PhysicalTargetId, OwnershipRestore>,
    ) -> Result<Self, AgentError> {
        Ok(Self {
            operation_kind: OperationKind::Rollback,
            parent_receipt_id: Some(parent_receipt_id),
            ownership_restores,
            project_root_identity: capture_project_root_identity(&plan.context)?,
            action_id: Some("rollback".into()),
        })
    }
}

pub(super) struct ClaimedPlan {
    pub(super) plan: MutationPlan,
    pub(super) intent: PlanExecutionIntent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanClaimBinding {
    pub context: super::AgentContext,
    pub risk_fingerprint: RiskFingerprint,
}

impl PlanState {
    fn prune_expired(&mut self, now: DateTime<Utc>) {
        self.active.retain(|_, stored| stored.plan.expires_at > now);
        self.consumed.retain(|_, expires_at| *expires_at > now);
    }
}

/// In-memory owner of mutation plans. Callers outside the backend only receive plan views.
#[derive(Default)]
pub struct PlanStore {
    state: Mutex<PlanState>,
}

impl PlanStore {
    pub fn claim_binding(&self, plan_id: &PlanId) -> Result<PlanClaimBinding, AgentError> {
        let state = self.state.lock().map_err(|_| lock_error())?;
        let stored = state.active.get(plan_id).ok_or_else(|| {
            plan_error(
                AgentErrorCode::InvalidPlan,
                None,
                None,
                "Unknown mutation plan",
                false,
            )
        })?;
        let view = MutationPlanView::from(&stored.plan);
        Ok(PlanClaimBinding {
            context: stored.plan.context.clone(),
            risk_fingerprint: view.risk_fingerprint,
        })
    }

    pub fn insert(&self, plan: MutationPlan) -> Result<MutationPlanView, AgentError> {
        self.insert_at(plan, Utc::now())
    }

    pub fn insert_confirmation_required(
        &self,
        plan: MutationPlan,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_confirmation_required_at(plan, Utc::now())
    }

    pub fn insert_with_acknowledgements(
        &self,
        plan: MutationPlan,
        requirements: Vec<AcknowledgementRequirement>,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_with_acknowledgements_at(plan, Utc::now(), requirements)
    }

    pub fn insert_conversion(
        &self,
        plan: MutationPlan,
        requirements: Vec<AcknowledgementRequirement>,
        report: ConversionReport,
    ) -> Result<MutationPlanView, AgentError> {
        let intent = PlanExecutionIntent::apply(&plan)?;
        self.insert_with_intent_at(
            plan,
            Utc::now(),
            requirements,
            intent,
            Some(Arc::new(report)),
        )
    }

    pub fn conversion_report(&self, plan_id: &PlanId) -> Result<ConversionReport, AgentError> {
        let report = {
            let state = self.state.lock().map_err(|_| lock_error())?;
            state
                .active
                .get(plan_id)
                .and_then(|stored| stored.conversion_report.clone())
                .ok_or_else(|| {
                    plan_error(
                        AgentErrorCode::InvalidPlan,
                        None,
                        None,
                        "Mutation plan is not a conversion plan",
                        false,
                    )
                })?
        };
        Ok((*report).clone())
    }

    pub fn insert_workspace_action(
        &self,
        plan: MutationPlan,
        workspace_key: &super::WorkspaceKey,
        resource_key: &super::ResourceKey,
        action: super::ResourceAction,
    ) -> Result<MutationPlanView, AgentError> {
        let actual_workspace =
            super::workspace_key_for_context(&plan.context).ok_or_else(|| {
                plan_error(
                    AgentErrorCode::InvalidPlan,
                    Some(&plan),
                    None,
                    "Workspace action plan has no canonical project identity",
                    false,
                )
            })?;
        if &actual_workspace != workspace_key {
            return Err(plan_error(
                AgentErrorCode::InvalidPlan,
                Some(&plan),
                None,
                "Workspace action plan identity changed during preview",
                false,
            ));
        }
        let mut intent = PlanExecutionIntent::apply(&plan)?;
        intent.action_id = Some(format!("{}:{}", action.contract_name(), resource_key));
        self.insert_with_intent_at(
            plan,
            Utc::now(),
            vec![AcknowledgementRequirement {
                code: PlanAcknowledgementCode::ProjectCollectionApply,
                risk: PlanRiskLevel::Confirmation,
            }],
            intent,
            None,
        )
    }

    pub(super) fn insert_rollback(
        &self,
        plan: MutationPlan,
        parent_receipt_id: ReceiptId,
        ownership_restores: BTreeMap<PhysicalTargetId, OwnershipRestore>,
    ) -> Result<MutationPlanView, AgentError> {
        let intent = PlanExecutionIntent::rollback(&plan, parent_receipt_id, ownership_restores)?;
        self.insert_with_intent_at(
            plan,
            Utc::now(),
            vec![AcknowledgementRequirement {
                code: PlanAcknowledgementCode::RollbackApply,
                risk: PlanRiskLevel::Confirmation,
            }],
            intent,
            None,
        )
    }

    pub(super) fn execution_intent(
        &self,
        plan_id: &PlanId,
    ) -> Result<PlanExecutionIntent, AgentError> {
        let state = self.state.lock().map_err(|_| lock_error())?;
        state
            .active
            .get(plan_id)
            .map(|stored| stored.intent.clone())
            .ok_or_else(|| {
                plan_error(
                    AgentErrorCode::InvalidPlan,
                    None,
                    None,
                    "Unknown mutation plan",
                    false,
                )
            })
    }

    pub(crate) fn resources_for_locking(
        &self,
        plan_id: &PlanId,
    ) -> Result<Vec<ResourceRef>, AgentError> {
        let state = self.state.lock().map_err(|_| lock_error())?;
        let stored = state.active.get(plan_id).ok_or_else(|| {
            plan_error(
                AgentErrorCode::InvalidPlan,
                None,
                None,
                "Unknown mutation plan",
                false,
            )
        })?;
        Ok(stored
            .plan
            .read_set
            .iter()
            .map(|precondition| precondition.resource.clone())
            .chain(
                stored
                    .plan
                    .mutations
                    .iter()
                    .map(|mutation| mutation.resource.clone()),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    pub(crate) fn resource_lifecycle_ids_for_locking(
        &self,
        plan_id: &PlanId,
    ) -> Result<Vec<String>, AgentError> {
        let state = self.state.lock().map_err(|_| lock_error())?;
        let stored = state.active.get(plan_id).ok_or_else(|| {
            plan_error(
                AgentErrorCode::InvalidPlan,
                None,
                None,
                "Unknown mutation plan",
                false,
            )
        })?;
        Ok(stored
            .plan
            .mutations
            .iter()
            .filter(|mutation| mutation.kind != super::MutationKind::Delete)
            .filter_map(|mutation| super::resource_lifecycle_id(&mutation.resource))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    pub fn claim_validated<F>(
        &self,
        plan_id: &PlanId,
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_validated_at(plan_id, Utc::now(), observe_digest)
    }

    pub fn claim_confirmed<F>(
        &self,
        plan_id: &PlanId,
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_confirmed_at(plan_id, Utc::now(), observe_digest)
    }

    pub fn claim_acknowledged<F>(
        &self,
        plan_id: &PlanId,
        acknowledgements: &[PlanAcknowledgement],
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_acknowledged_at(plan_id, Utc::now(), acknowledgements, observe_digest)
    }

    pub(super) fn claim_acknowledged_for_execution<F>(
        &self,
        plan_id: &PlanId,
        expected: &PlanClaimBinding,
        acknowledgements: &[PlanAcknowledgement],
        observe_digest: F,
    ) -> Result<ClaimedPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_with_intent_at(
            plan_id,
            Utc::now(),
            Some(expected),
            acknowledgements,
            observe_digest,
        )
    }

    fn insert_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_with_acknowledgements_at(plan, now, Vec::new())
    }

    fn insert_confirmation_required_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_with_acknowledgements_at(
            plan,
            now,
            vec![AcknowledgementRequirement {
                code: PlanAcknowledgementCode::ConversionApply,
                risk: PlanRiskLevel::Confirmation,
            }],
        )
    }

    fn insert_with_acknowledgements_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
        requirements: Vec<AcknowledgementRequirement>,
    ) -> Result<MutationPlanView, AgentError> {
        let intent = PlanExecutionIntent::apply(&plan)?;
        self.insert_with_intent_at(plan, now, requirements, intent, None)
    }

    fn insert_with_intent_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
        requirements: Vec<AcknowledgementRequirement>,
        intent: PlanExecutionIntent,
        conversion_report: Option<Arc<ConversionReport>>,
    ) -> Result<MutationPlanView, AgentError> {
        plan.validate()?;
        if plan.expires_at <= now {
            return Err(plan_error(
                AgentErrorCode::PlanExpired,
                Some(&plan),
                None,
                "Mutation plan has already expired",
                true,
            ));
        }
        let mut view = MutationPlanView::from(&plan);
        view.required_acknowledgements = requirements.clone();
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        state.prune_expired(now);
        if state.active.contains_key(&plan.id) || state.consumed.contains_key(&plan.id) {
            return Err(plan_error(
                AgentErrorCode::InvalidPlan,
                Some(&plan),
                None,
                "Mutation plan id already exists",
                false,
            ));
        }
        state.active.insert(
            plan.id.clone(),
            StoredPlan {
                plan,
                required_acknowledgements: requirements,
                intent,
                conversion_report,
            },
        );
        Ok(view)
    }

    fn claim_validated_at<F>(
        &self,
        plan_id: &PlanId,
        now: DateTime<Utc>,
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_acknowledged_at(plan_id, now, &[], observe_digest)
    }

    fn claim_confirmed_at<F>(
        &self,
        plan_id: &PlanId,
        now: DateTime<Utc>,
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_acknowledged_at(
            plan_id,
            now,
            &[PlanAcknowledgement {
                code: PlanAcknowledgementCode::ConversionApply,
                accepted: true,
            }],
            observe_digest,
        )
    }

    fn claim_acknowledged_at<F>(
        &self,
        plan_id: &PlanId,
        now: DateTime<Utc>,
        acknowledgements: &[PlanAcknowledgement],
        observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        self.claim_with_intent_at(plan_id, now, None, acknowledgements, observe_digest)
            .map(|claimed| claimed.plan)
    }

    fn claim_with_intent_at<F>(
        &self,
        plan_id: &PlanId,
        now: DateTime<Utc>,
        expected: Option<&PlanClaimBinding>,
        acknowledgements: &[PlanAcknowledgement],
        mut observe_digest: F,
    ) -> Result<ClaimedPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        let claimed = {
            let mut state = self.state.lock().map_err(|_| lock_error())?;
            if matches!(
                state.active.get(plan_id),
                Some(stored) if stored.plan.expires_at <= now
            ) {
                let expired = state
                    .active
                    .remove(plan_id)
                    .expect("expired plan existence checked")
                    .plan;
                state.prune_expired(now);
                return Err(plan_error(
                    AgentErrorCode::PlanExpired,
                    Some(&expired),
                    None,
                    "Mutation plan has expired",
                    true,
                ));
            }
            state.prune_expired(now);
            if state.consumed.contains_key(plan_id) {
                return Err(plan_error(
                    AgentErrorCode::InvalidPlan,
                    None,
                    None,
                    "Mutation plan has already been consumed",
                    false,
                ));
            }
            let stored = state.active.get(plan_id).ok_or_else(|| {
                plan_error(
                    AgentErrorCode::InvalidPlan,
                    None,
                    None,
                    "Unknown mutation plan",
                    false,
                )
            })?;
            if let Some(expected) = expected {
                let actual_risk = MutationPlanView::from(&stored.plan).risk_fingerprint;
                if stored.plan.context != expected.context
                    || actual_risk != expected.risk_fingerprint
                {
                    return Err(plan_error(
                        AgentErrorCode::ResourceChanged,
                        Some(&stored.plan),
                        None,
                        "Mutation plan no longer matches the expected workspace context",
                        true,
                    ));
                }
            }
            let required = stored
                .required_acknowledgements
                .iter()
                .map(|requirement| requirement.code)
                .collect::<BTreeSet<_>>();
            let accepted = acknowledgements
                .iter()
                .filter(|acknowledgement| acknowledgement.accepted)
                .map(|acknowledgement| acknowledgement.code)
                .collect::<BTreeSet<_>>();
            let exact = acknowledgements.len() == accepted.len() && required == accepted;
            if !exact {
                return Err(plan_error(
                    AgentErrorCode::ConfirmationRequired,
                    Some(&stored.plan),
                    None,
                    "Mutation plan acknowledgements do not match its requirements",
                    false,
                ));
            }
            let stored = state
                .active
                .remove(plan_id)
                .expect("plan existence checked");
            state
                .consumed
                .insert(plan_id.clone(), stored.plan.expires_at);
            ClaimedPlan {
                plan: stored.plan,
                intent: stored.intent,
            }
        };

        if claimed.plan.expires_at <= now {
            return Err(plan_error(
                AgentErrorCode::PlanExpired,
                Some(&claimed.plan),
                None,
                "Mutation plan has expired",
                true,
            ));
        }

        for precondition in &claimed.plan.read_set {
            let actual = observe_digest(&precondition.resource)?;
            ensure_digest(
                &claimed.plan,
                &precondition.resource,
                Some(&precondition.expected_digest),
                actual.as_ref(),
            )?;
        }
        for mutation in &claimed.plan.mutations {
            let actual = observe_digest(&mutation.resource)?;
            ensure_digest(
                &claimed.plan,
                &mutation.resource,
                mutation.expected_digest.as_ref(),
                actual.as_ref(),
            )?;
        }

        Ok(claimed)
    }
}

fn ensure_digest(
    plan: &MutationPlan,
    resource: &ResourceRef,
    expected: Option<&ContentDigest>,
    actual: Option<&ContentDigest>,
) -> Result<(), AgentError> {
    if expected == actual {
        return Ok(());
    }

    Err(AgentError {
        code: AgentErrorCode::ResourceChanged,
        message: format!(
            "Resource {} changed after the plan was created",
            resource.logical_id
        ),
        agent_id: Some(plan.agent_id.clone()),
        installation_id: Some(resource.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: true,
        details: Some(serde_json::json!({
            "expectedDigest": expected.map(ContentDigest::as_str),
            "actualDigest": actual.map(ContentDigest::as_str),
        })),
    })
}

fn plan_error(
    code: AgentErrorCode,
    plan: Option<&MutationPlan>,
    resource: Option<ResourceRef>,
    message: impl Into<String>,
    retryable: bool,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: plan.map(|plan| plan.agent_id.clone()),
        installation_id: plan.map(|plan| plan.context.installation_id.clone()),
        resource,
        retryable,
        details: None,
    }
}

fn lock_error() -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: "Mutation plan store lock is poisoned".into(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: true,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::super::{
        AcknowledgementRequirement, AgentContext, AgentErrorCode, AgentId, ContentDigest,
        InstallationId, MutationKind, MutationPlan, PlanAcknowledgement, PlanAcknowledgementCode,
        PlanId, PlanRiskLevel, PlannedMutation, ReadPrecondition, ResourceKind, ResourceRef,
        ResourceScope, WritePolicy,
    };
    use super::{PlanClaimBinding, PlanStore};

    fn resource() -> ResourceRef {
        ResourceRef {
            installation_id: InstallationId::from("claude:default"),
            project_path: None,
            kind: ResourceKind::Settings,
            scope: ResourceScope::User,
            logical_id: "user-settings".into(),
        }
    }

    fn plan(now: chrono::DateTime<Utc>) -> MutationPlan {
        MutationPlan {
            id: PlanId::from("plan-1"),
            agent_id: AgentId::from("claude-code"),
            context: AgentContext {
                installation_id: InstallationId::from("claude:default"),
                project_path: None,
            },
            read_set: Vec::new(),
            mutations: vec![PlannedMutation {
                resource: resource(),
                kind: MutationKind::Replace,
                expected_digest: Some(ContentDigest::from("sha256:before")),
                media_type: "application/json".into(),
                content: Some(serde_json::json!({"model": "new"})),
            }],
            expires_at: now + Duration::minutes(5),
        }
    }

    #[test]
    fn plan_view_omits_backend_owned_content() {
        let now = Utc::now();
        let store = PlanStore::default();

        let view = store.insert_at(plan(now), now).unwrap();
        let json = serde_json::to_value(view).unwrap();

        assert_eq!(json["changes"][0]["kind"], "replace");
        assert!(json["changes"][0].get("content").is_none());
        assert!(json.get("readSet").is_none());
    }

    #[test]
    fn expired_plan_cannot_be_claimed() {
        let now = Utc::now();
        let store = PlanStore::default();
        store.insert_at(plan(now), now).unwrap();

        let error = store
            .claim_validated_at(&PlanId::from("plan-1"), now + Duration::minutes(6), |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::PlanExpired);
    }

    #[test]
    fn insertion_prunes_expired_active_and_consumed_plans() {
        let now = Utc::now();
        let store = PlanStore::default();
        store.insert_at(plan(now), now).unwrap();
        store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap();
        let later = now + Duration::minutes(6);
        let mut replacement = plan(later);
        replacement.id = PlanId::from("plan-2");

        store.insert_at(replacement, later).unwrap();

        let state = store.state.lock().unwrap();
        assert_eq!(state.active.len(), 1);
        assert!(state.active.contains_key(&PlanId::from("plan-2")));
        assert!(state.consumed.is_empty());
    }

    #[test]
    fn unknown_plan_is_rejected() {
        let store = PlanStore::default();

        let error = store
            .claim_validated_at(&PlanId::from("missing"), Utc::now(), |_| Ok(None))
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::InvalidPlan);
        assert!(error.message.contains("Unknown"));
    }

    #[test]
    fn claimed_plan_cannot_be_replayed() {
        let now = Utc::now();
        let store = PlanStore::default();
        store.insert_at(plan(now), now).unwrap();

        store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap();
        let error = store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| Ok(None))
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::InvalidPlan);
        assert!(error.message.contains("already been consumed"));
    }

    #[test]
    fn changed_target_invalidates_and_consumes_plan() {
        let now = Utc::now();
        let store = PlanStore::default();
        store.insert_at(plan(now), now).unwrap();

        let changed = store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| {
                Ok(Some(ContentDigest::from("sha256:external-change")))
            })
            .unwrap_err();
        let replay = store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| Ok(None))
            .unwrap_err();

        assert_eq!(changed.code, AgentErrorCode::ResourceChanged);
        assert!(changed.retryable);
        assert!(changed.resource.is_some());
        assert!(replay.message.contains("already been consumed"));
    }

    #[test]
    fn changed_read_resource_invalidates_plan() {
        let now = Utc::now();
        let store = PlanStore::default();
        let mut pending = plan(now);
        let mut source = resource();
        source.logical_id = "conversion-source".into();
        pending.read_set.push(ReadPrecondition {
            resource: source,
            expected_digest: ContentDigest::from("sha256:source-before"),
            write_policy: WritePolicy::ReadOnly,
        });
        store.insert_at(pending, now).unwrap();

        let error = store
            .claim_validated_at(&PlanId::from("plan-1"), now, |resource| {
                if resource.logical_id == "conversion-source" {
                    Ok(Some(ContentDigest::from("sha256:source-changed")))
                } else {
                    Ok(Some(ContentDigest::from("sha256:before")))
                }
            })
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::ResourceChanged);
        assert_eq!(
            error.resource.unwrap().logical_id,
            "conversion-source".to_string()
        );
    }

    #[test]
    fn conversion_plan_requires_the_confirmed_claim_path() {
        let now = Utc::now();
        let store = PlanStore::default();
        store
            .insert_confirmation_required_at(plan(now), now)
            .unwrap();

        let unconfirmed = store
            .claim_validated_at(&PlanId::from("plan-1"), now, |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap_err();
        let confirmed = store
            .claim_confirmed_at(&PlanId::from("plan-1"), now, |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap();

        assert_eq!(unconfirmed.code, AgentErrorCode::ConfirmationRequired);
        assert!(unconfirmed.message.contains("acknowledgements"));
        assert_eq!(confirmed.id.as_str(), "plan-1");
    }

    #[test]
    fn acknowledgement_requirements_are_exposed_without_plan_content() {
        let now = Utc::now();
        let store = PlanStore::default();

        let view = store
            .insert_with_acknowledgements_at(
                plan(now),
                now,
                vec![AcknowledgementRequirement {
                    code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                    risk: PlanRiskLevel::Dangerous,
                }],
            )
            .unwrap();
        let json = serde_json::to_value(view).unwrap();

        assert_eq!(
            json["requiredAcknowledgements"][0]["code"],
            "dangerous_permission_expansion"
        );
        assert_eq!(json["requiredAcknowledgements"][0]["risk"], "dangerous");
        assert!(json.get("readSet").is_none());
    }

    #[test]
    fn acknowledgement_set_must_match_and_failed_attempt_does_not_consume_plan() {
        let now = Utc::now();
        let store = PlanStore::default();
        let requirement = AcknowledgementRequirement {
            code: PlanAcknowledgementCode::DangerousPermissionExpansion,
            risk: PlanRiskLevel::Dangerous,
        };
        store
            .insert_with_acknowledgements_at(plan(now), now, vec![requirement])
            .unwrap();

        let missing = store
            .claim_acknowledged_at(&PlanId::from("plan-1"), now, &[], |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap_err();
        let accepted = store
            .claim_acknowledged_at(
                &PlanId::from("plan-1"),
                now,
                &[PlanAcknowledgement {
                    code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                    accepted: true,
                }],
                |_| Ok(Some(ContentDigest::from("sha256:before"))),
            )
            .unwrap();

        assert_eq!(missing.code, AgentErrorCode::ConfirmationRequired);
        assert_eq!(accepted.id.as_str(), "plan-1");
    }

    #[test]
    fn unknown_acknowledgement_cannot_replace_the_required_risk() {
        let now = Utc::now();
        let store = PlanStore::default();
        store
            .insert_with_acknowledgements_at(
                plan(now),
                now,
                vec![AcknowledgementRequirement {
                    code: PlanAcknowledgementCode::DangerousPermissionExpansion,
                    risk: PlanRiskLevel::Dangerous,
                }],
            )
            .unwrap();

        let error = store
            .claim_acknowledged_at(
                &PlanId::from("plan-1"),
                now,
                &[PlanAcknowledgement {
                    code: PlanAcknowledgementCode::ConversionApply,
                    accepted: true,
                }],
                |_| Ok(Some(ContentDigest::from("sha256:before"))),
            )
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::ConfirmationRequired);
    }

    #[test]
    fn execution_claim_requires_the_previewed_context_and_risk() {
        let now = Utc::now();
        let store = PlanStore::default();
        store.insert_at(plan(now), now).unwrap();
        let expected = store.claim_binding(&PlanId::from("plan-1")).unwrap();
        let stale = PlanClaimBinding {
            context: AgentContext {
                installation_id: InstallationId::from("claude:other"),
                project_path: None,
            },
            risk_fingerprint: expected.risk_fingerprint.clone(),
        };

        let error = store
            .claim_acknowledged_for_execution(&PlanId::from("plan-1"), &stale, &[], |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .err()
            .unwrap();
        let claimed = store
            .claim_acknowledged_for_execution(&PlanId::from("plan-1"), &expected, &[], |_| {
                Ok(Some(ContentDigest::from("sha256:before")))
            })
            .unwrap();

        assert_eq!(error.code, AgentErrorCode::ResourceChanged);
        assert!(error.retryable);
        assert_eq!(claimed.plan.id.as_str(), "plan-1");
    }
}

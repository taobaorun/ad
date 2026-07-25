use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use super::{
    AcknowledgementRequirement, AgentError, AgentErrorCode, ContentDigest, MutationPlan,
    MutationPlanView, PlanAcknowledgement, PlanAcknowledgementCode, PlanId, PlanRiskLevel,
    ResourceRef,
};

#[derive(Default)]
struct PlanState {
    active: HashMap<PlanId, StoredPlan>,
    consumed: HashMap<PlanId, DateTime<Utc>>,
}

struct StoredPlan {
    plan: MutationPlan,
    required_acknowledgements: Vec<AcknowledgementRequirement>,
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
        mut observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        let plan = {
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
            stored.plan
        };

        if plan.expires_at <= now {
            return Err(plan_error(
                AgentErrorCode::PlanExpired,
                Some(&plan),
                None,
                "Mutation plan has expired",
                true,
            ));
        }

        for precondition in &plan.read_set {
            let actual = observe_digest(&precondition.resource)?;
            ensure_digest(
                &plan,
                &precondition.resource,
                Some(&precondition.expected_digest),
                actual.as_ref(),
            )?;
        }
        for mutation in &plan.mutations {
            let actual = observe_digest(&mutation.resource)?;
            ensure_digest(
                &plan,
                &mutation.resource,
                mutation.expected_digest.as_ref(),
                actual.as_ref(),
            )?;
        }

        Ok(plan)
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
    use super::PlanStore;

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
}

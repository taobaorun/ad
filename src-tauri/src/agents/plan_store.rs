use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use super::{
    AgentError, AgentErrorCode, ContentDigest, MutationPlan, MutationPlanView, PlanId, ResourceRef,
};

#[derive(Default)]
struct PlanState {
    active: HashMap<PlanId, StoredPlan>,
    consumed: HashSet<PlanId>,
}

struct StoredPlan {
    plan: MutationPlan,
    confirmation_required: bool,
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

    fn insert_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_with_confirmation_at(plan, now, false)
    }

    fn insert_confirmation_required_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
    ) -> Result<MutationPlanView, AgentError> {
        self.insert_with_confirmation_at(plan, now, true)
    }

    fn insert_with_confirmation_at(
        &self,
        plan: MutationPlan,
        now: DateTime<Utc>,
        confirmation_required: bool,
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
        let view = MutationPlanView::from(&plan);
        let mut state = self.state.lock().map_err(|_| lock_error())?;
        if state.active.contains_key(&plan.id) || state.consumed.contains(&plan.id) {
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
                confirmation_required,
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
        self.claim_with_confirmation_at(plan_id, now, false, observe_digest)
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
        self.claim_with_confirmation_at(plan_id, now, true, observe_digest)
    }

    fn claim_with_confirmation_at<F>(
        &self,
        plan_id: &PlanId,
        now: DateTime<Utc>,
        confirmed: bool,
        mut observe_digest: F,
    ) -> Result<MutationPlan, AgentError>
    where
        F: FnMut(&ResourceRef) -> Result<Option<ContentDigest>, AgentError>,
    {
        let plan = {
            let mut state = self.state.lock().map_err(|_| lock_error())?;
            if state.consumed.contains(plan_id) {
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
            if stored.confirmation_required && !confirmed {
                return Err(plan_error(
                    AgentErrorCode::PermissionDenied,
                    Some(&stored.plan),
                    None,
                    "Mutation plan requires explicit confirmation",
                    false,
                ));
            }
            let plan = state
                .active
                .remove(plan_id)
                .expect("plan existence checked")
                .plan;
            state.consumed.insert(plan_id.clone());
            plan
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
        AgentContext, AgentErrorCode, AgentId, ContentDigest, InstallationId, MutationKind,
        MutationPlan, PlanId, PlannedMutation, ReadPrecondition, ResourceKind, ResourceRef,
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

        assert_eq!(unconfirmed.code, AgentErrorCode::PermissionDenied);
        assert!(unconfirmed.message.contains("confirmation"));
        assert_eq!(confirmed.id.as_str(), "plan-1");
    }
}

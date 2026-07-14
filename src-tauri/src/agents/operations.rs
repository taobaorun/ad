use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AgentContext, AgentId, InstallationId, PlanId, ReceiptId};

/// Stable digest of observed resource content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ContentDigest {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ContentDigest {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Agent-independent resource categories understood by the application core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Settings,
    Instructions,
    Skills,
    Plugins,
    Hooks,
    Mcp,
    Agents,
    Rules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceScope {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOrigin {
    User,
    Project,
    System,
}

/// Logical identity of one resource owned by an Agent installation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    pub installation_id: InstallationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    pub kind: ResourceKind,
    pub scope: ResourceScope,
    pub logical_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLocation {
    pub path: String,
    pub origin: ResourceOrigin,
}

/// Immutable observation returned by an adapter inspect operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    pub resource: ResourceRef,
    pub location: ResourceLocation,
    pub media_type: String,
    pub content: Value,
    pub digest: ContentDigest,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePolicy {
    ReadOnly,
    Mutable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPrecondition {
    pub resource: ResourceRef,
    pub expected_digest: ContentDigest,
    pub write_policy: WritePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    Create,
    Replace,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedMutation {
    pub resource: ResourceRef,
    pub kind: MutationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_digest: Option<ContentDigest>,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
}

/// Backend-owned write plan. Adapters may construct plans but never apply them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationPlan {
    pub id: PlanId,
    pub agent_id: AgentId,
    pub context: AgentContext,
    #[serde(default)]
    pub read_set: Vec<ReadPrecondition>,
    #[serde(default)]
    pub mutations: Vec<PlannedMutation>,
    pub expires_at: DateTime<Utc>,
}

impl MutationPlan {
    pub fn validate(&self) -> Result<(), AgentError> {
        for mutation in &self.mutations {
            let read_only = self.read_set.iter().any(|precondition| {
                precondition.resource == mutation.resource
                    && precondition.write_policy == WritePolicy::ReadOnly
            });
            if read_only {
                return Err(AgentError::invalid_plan(
                    self.agent_id.clone(),
                    mutation.resource.clone(),
                    format!("Resource {} is read-only", mutation.resource.logical_id),
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Complete,
    Compensated,
    PartialFailure,
}

/// Durable outcome of applying a mutation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationReceipt {
    pub id: ReceiptId,
    pub plan_id: PlanId,
    pub status: OperationStatus,
    #[serde(default)]
    pub applied_resources: Vec<ResourceRef>,
    #[serde(default)]
    pub backup_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorCode {
    InvalidPlan,
    ResourceChanged,
    PermissionDenied,
    Unsupported,
    PlanExpired,
    PartialFailure,
    Io,
}

/// Structured error contract shared by adapter ports and the execution engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentError {
    pub code: AgentErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<InstallationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl AgentError {
    fn invalid_plan(agent_id: AgentId, resource: ResourceRef, message: String) -> Self {
        Self {
            code: AgentErrorCode::InvalidPlan,
            message,
            agent_id: Some(agent_id),
            installation_id: Some(resource.installation_id.clone()),
            resource: Some(resource),
            retryable: false,
            details: None,
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl fmt::Display for AgentErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        formatter.write_str(value.as_str().ok_or(fmt::Error)?)
    }
}

impl std::error::Error for AgentError {}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::agents::{AgentContext, AgentId, InstallationId, PlanId, ReceiptId};

    fn settings_resource() -> ResourceRef {
        ResourceRef {
            installation_id: InstallationId::from("codex:default"),
            project_path: Some("/Users/test/project".into()),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "project-config".into(),
        }
    }

    #[test]
    fn resource_snapshot_round_trips_without_agent_specific_fields() {
        let snapshot = ResourceSnapshot {
            resource: settings_resource(),
            location: ResourceLocation {
                path: "/Users/test/project/.codex/config.toml".into(),
                origin: ResourceOrigin::Project,
            },
            media_type: "application/toml".into(),
            content: serde_json::Value::String("model = \"gpt-5.4\"\n".into()),
            digest: ContentDigest::from("sha256:abc"),
            observed_at: Utc::now(),
        };

        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["resource"]["kind"], "settings");
        assert_eq!(json["location"]["origin"], "project");
        assert!(json.get("claudeSettings").is_none());
        assert!(json.get("codexConfig").is_none());
        assert_eq!(
            serde_json::from_value::<ResourceSnapshot>(json).unwrap(),
            snapshot
        );
    }

    #[test]
    fn mutation_plan_rejects_writes_to_read_only_resources() {
        let source = settings_resource();
        let plan = MutationPlan {
            id: PlanId::from("plan-1"),
            agent_id: AgentId::from("codex"),
            context: AgentContext {
                installation_id: InstallationId::from("codex:default"),
                project_path: Some("/Users/test/project".into()),
            },
            read_set: vec![ReadPrecondition {
                resource: source.clone(),
                expected_digest: ContentDigest::from("sha256:source"),
                write_policy: WritePolicy::ReadOnly,
            }],
            mutations: vec![PlannedMutation {
                resource: source,
                kind: MutationKind::Replace,
                expected_digest: Some(ContentDigest::from("sha256:source")),
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String("model = \"gpt-5.4\"\n".into())),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        };

        let error = plan.validate().unwrap_err();
        assert_eq!(error.code, AgentErrorCode::InvalidPlan);
        assert!(error.message.contains("read-only"));
    }

    #[test]
    fn partial_failure_receipt_has_a_stable_ipc_shape() {
        let receipt = OperationReceipt {
            id: ReceiptId::from("receipt-1"),
            plan_id: PlanId::from("plan-1"),
            status: OperationStatus::PartialFailure,
            applied_resources: vec![settings_resource()],
            backup_paths: vec!["/Users/test/.ad/backups/config.toml".into()],
            message: Some("A compensation write failed".into()),
        };

        let json = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json["status"], "partial_failure");
        assert_eq!(json["planId"], "plan-1");
        assert_eq!(
            serde_json::from_value::<OperationReceipt>(json).unwrap(),
            receipt
        );
    }
}

use serde::{Deserialize, Serialize};

use super::{
    AgentId, CapabilityLimitation, DeclarationKey, OwnershipRecordId, PhysicalTargetId,
    ResourceKey, ResourceKind, ResourceLayer, ResourceScope, WorkspaceKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDiagnostic {
    pub code: String,
    pub message_key: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<ResourceKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCoverage {
    pub status: CoverageStatus,
    pub observed: usize,
    pub visible: usize,
    #[serde(default)]
    pub diagnostics: Vec<ItemDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveResourceState {
    Enabled,
    Disabled,
    Unconfigured,
    Conflict,
    External,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDeclarationView {
    pub key: DeclarationKey,
    pub layer: ResourceLayer,
    pub source_id: String,
    pub target_id: PhysicalTargetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ResourceScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProvenanceView {
    #[serde(default)]
    pub declarations: Vec<ResourceDeclarationView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<DeclarationKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnershipKind {
    AdManaged,
    AgentManaged,
    External,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceOwnershipView {
    pub kind: ResourceOwnershipKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<OwnershipRecordId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceHealthStatus {
    Healthy,
    Degraded,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHealthView {
    pub status: ResourceHealthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<ItemDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAction {
    Inspect,
    Edit,
    Install,
    Update,
    Remove,
    Enable,
    Disable,
    Convert,
    OpenExternal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionAvailability {
    Available,
    ConfirmationRequired,
    Unavailable,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceActionView {
    pub action: ResourceAction,
    pub availability: ResourceActionAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limitation: Option<CapabilityLimitation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceManagementStatus {
    Managed,
    ReadOnly,
    External,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceManagementView {
    pub status: ResourceManagementStatus,
    #[serde(default)]
    pub actions: Vec<ResourceActionView>,
}

/// Backend-resolved effective collection item. Raw snapshots remain separate observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResourceView {
    pub key: ResourceKey,
    pub kind: ResourceKind,
    pub logical_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub effective_state: EffectiveResourceState,
    pub provenance: ResourceProvenanceView,
    pub ownership: ResourceOwnershipView,
    pub health: ResourceHealthView,
    pub management: ResourceManagementView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResourceInventory {
    pub workspace_key: WorkspaceKey,
    pub agent_id: AgentId,
    pub kind: ResourceKind,
    pub coverage: CategoryCoverage,
    #[serde(default)]
    pub resources: Vec<CollectionResourceView>,
}

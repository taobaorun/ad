use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AgentId, CapabilityLimitation, DeclarationKey, InventoryRevision, OwnershipRecordId,
    PhysicalTargetId, ResourceKey, ResourceKind, ResourceLayer, ResourceRef, ResourceScope,
    UserWorkspaceDescriptor, WorkspaceDescriptor, WorkspaceKey,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceSourceKind {
    CatalogGit,
    CatalogLocal,
    InstalledPath,
}

/// Display-only source provenance. Mutation requests continue to use opaque resource keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSourceView {
    pub kind: ResourceSourceKind,
    pub display_name: String,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProvenanceView {
    #[serde(default)]
    pub declarations: Vec<ResourceDeclarationView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<DeclarationKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ResourceSourceView>,
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

impl ResourceAction {
    pub(crate) fn contract_name(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Edit => "edit",
            Self::Install => "install",
            Self::Update => "update",
            Self::Remove => "remove",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Convert => "convert",
            Self::OpenExternal => "open_external",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionAvailability {
    Available,
    ConfirmationRequired,
    Unavailable,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceActionIntent {
    Standard,
    Relink,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceActionView {
    pub action: ResourceAction,
    pub intent: ResourceActionIntent,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryCompatibility {
    Verified,
    Unverified,
}

/// Versioned compatibility boundary used to decide whether inventory may claim completeness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterDiscoveryContract {
    pub adapter_version: u32,
    pub location_set: String,
    #[serde(default)]
    pub schema_versions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_agent_version: Option<String>,
    #[serde(default)]
    pub verified_agent_versions: Vec<String>,
    pub compatibility: DiscoveryCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsValueSensitivity {
    Public,
    Sensitive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFieldDeclarationView {
    pub declaration_key: DeclarationKey,
    pub layer: ResourceLayer,
    pub value: Value,
    pub sensitivity: SettingsValueSensitivity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFieldView {
    pub path: String,
    pub value: Value,
    pub sensitivity: SettingsValueSensitivity,
    #[serde(default)]
    pub declarations: Vec<SettingsFieldDeclarationView>,
    pub winner: DeclarationKey,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLayerView {
    pub declaration: ResourceDeclarationView,
    pub logical_id: String,
    pub media_type: String,
    pub content: Value,
    pub exists: bool,
    pub editable: bool,
    pub preserves_unknown_fields: bool,
    #[serde(default)]
    pub redacted_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEditableTargetView {
    pub declaration_key: DeclarationKey,
    pub resource: ResourceRef,
    pub media_type: String,
    pub exists: bool,
    pub preserves_unknown_fields: bool,
    #[serde(default)]
    pub redacted_paths: Vec<String>,
}

/// Backend-resolved effective Settings view. All sensitive values are masked before IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEffectiveView {
    pub workspace_key: WorkspaceKey,
    pub coverage: CategoryCoverage,
    pub effective_content: Value,
    #[serde(default)]
    pub fields: Vec<SettingsFieldView>,
    #[serde(default)]
    pub layers: Vec<SettingsLayerView>,
    #[serde(default)]
    pub editable_targets: Vec<SettingsEditableTargetView>,
}

/// One coherent, revision-bound read of a project's effective Agent configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkspaceInventory {
    pub schema_version: u32,
    pub workspace: WorkspaceDescriptor,
    pub revision: InventoryRevision,
    pub discovery: AdapterDiscoveryContract,
    pub settings: SettingsEffectiveView,
    pub skills: CollectionResourceInventory,
    pub plugins: CollectionResourceInventory,
    #[serde(default)]
    pub diagnostics: Vec<ItemDiagnostic>,
}

/// One revision-bound read of a selected Agent installation's user resources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResourceInventory {
    pub schema_version: u32,
    pub workspace: UserWorkspaceDescriptor,
    pub revision: InventoryRevision,
    pub skills: CollectionResourceInventory,
    pub plugins: CollectionResourceInventory,
    #[serde(default)]
    pub diagnostics: Vec<ItemDiagnostic>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn resource_provenance_serializes_display_only_source_metadata() {
        let provenance = ResourceProvenanceView {
            declarations: Vec::new(),
            winner: None,
            source: Some(ResourceSourceView {
                kind: ResourceSourceKind::CatalogGit,
                display_name: "Team skills".into(),
                location: "https://example.com/team/skills.git".into(),
                branch: Some("main".into()),
                subdirectory: Some("skills/review".into()),
            }),
        };

        assert_eq!(
            serde_json::to_value(provenance).unwrap(),
            json!({
                "declarations": [],
                "source": {
                    "kind": "catalog_git",
                    "displayName": "Team skills",
                    "location": "https://example.com/team/skills.git",
                    "branch": "main",
                    "subdirectory": "skills/review"
                }
            })
        );
    }

    #[test]
    fn resource_provenance_omits_source_when_unavailable() {
        let provenance = ResourceProvenanceView {
            declarations: Vec::new(),
            winner: None,
            source: None,
        };

        let value = serde_json::to_value(provenance).unwrap();

        assert!(value.get("source").is_none());
    }
}

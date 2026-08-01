use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(AgentId, "Stable identifier for a built-in Agent adapter.");
string_id!(
    InstallationId,
    "Stable identifier for one canonical Agent configuration instance."
);
string_id!(ProfileId, "Profile identifier scoped by AgentId.");
string_id!(PlanId, "Identifier for a backend-owned mutation plan.");
string_id!(ReceiptId, "Identifier for an applied operation receipt.");
string_id!(
    WorkspaceKey,
    "Opaque identity for one selected project and Agent context."
);
string_id!(
    WorkspaceRevision,
    "Opaque revision binding a workspace descriptor to its backend inputs."
);
string_id!(
    ResourceKey,
    "Stable effective identity for a workspace resource."
);
string_id!(
    DeclarationKey,
    "Stable identity for one layer declaration of a resource."
);
string_id!(
    PhysicalTargetId,
    "Opaque identity for a backend-resolved physical mutation target."
);
string_id!(
    OwnershipRecordId,
    "Opaque identity for an AD resource ownership record."
);
string_id!(
    RiskFingerprint,
    "Opaque fingerprint of the public risk-relevant plan shape."
);

/// Static definition for one built-in Agent product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub id: AgentId,
    pub display_name: String,
    pub adapter_version: u32,
}

/// Concrete target for Agent operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContext {
    pub installation_id: InstallationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
}

/// Capabilities exposed by an Agent adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Settings,
    Skills,
    Plugins,
    ProcessDetection,
    TerminalLaunch,
    Conversion,
}

/// Static metadata registered by a built-in adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMetadata {
    pub id: AgentId,
    pub display_name: String,
    #[serde(default)]
    pub capabilities: BTreeSet<Capability>,
}

/// A canonical Agent installation discovered on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallation {
    pub id: InstallationId,
    pub agent_id: AgentId,
    pub root_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_installation_id: Option<InstallationId>,
}

/// Optional Project Runtime selected as the effective installation for a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntimeIdentity {
    pub installation_id: InstallationId,
    pub base_installation_id: InstallationId,
    pub revision: WorkspaceRevision,
}

/// Backend-created identity for one selected Agent context in a canonical project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDescriptor {
    pub schema_version: u32,
    pub key: WorkspaceKey,
    pub revision: WorkspaceRevision,
    pub agent_id: AgentId,
    pub canonical_project_path: String,
    pub base_installation_id: InstallationId,
    pub effective_installation_id: InstallationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_runtime: Option<ProjectRuntimeIdentity>,
}

impl WorkspaceDescriptor {
    pub fn for_installation(
        canonical_project_path: &str,
        installation: &AgentInstallation,
        project_runtime: Option<ProjectRuntimeIdentity>,
    ) -> Self {
        let base_installation_id = installation
            .base_installation_id
            .clone()
            .or_else(|| {
                project_runtime
                    .as_ref()
                    .map(|runtime| runtime.base_installation_id.clone())
            })
            .unwrap_or_else(|| installation.id.clone());
        let effective_installation_id = project_runtime
            .as_ref()
            .map(|runtime| runtime.installation_id.clone())
            .unwrap_or_else(|| installation.id.clone());
        let key = WorkspaceKey::from(opaque_contract_id(
            "workspace",
            &[
                canonical_project_path,
                installation.agent_id.as_str(),
                base_installation_id.as_str(),
            ],
        ));
        let runtime_revision = project_runtime
            .as_ref()
            .map(|runtime| runtime.revision.as_str())
            .unwrap_or("none");
        let revision = WorkspaceRevision::from(opaque_contract_id(
            "workspace-revision",
            &[
                key.as_str(),
                effective_installation_id.as_str(),
                runtime_revision,
            ],
        ));
        Self {
            schema_version: 1,
            key,
            revision,
            agent_id: installation.agent_id.clone(),
            canonical_project_path: canonical_project_path.to_owned(),
            base_installation_id,
            effective_installation_id,
            project_runtime,
        }
    }
}

impl AgentInstallation {
    pub(crate) fn with_id(
        id: impl Into<InstallationId>,
        agent_id: impl Into<AgentId>,
        root_path: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            agent_id: agent_id.into(),
            root_path: normalize_root_path(&root_path.into()),
            project_path: None,
            base_installation_id: None,
        }
    }
}

/// Composite identity for a profile belonging to one Agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileRef {
    pub agent_id: AgentId,
    pub profile_id: ProfileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionIssueKind {
    Unsupported,
    RequiresConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionIssue {
    pub path: String,
    pub kind: ConversionIssueKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPreview {
    pub source_agent_id: AgentId,
    pub target_agent_id: AgentId,
    pub target_format: String,
    pub target_content: String,
    #[serde(default)]
    pub issues: Vec<ConversionIssue>,
}

impl AgentProfileRef {
    pub fn new(agent_id: impl Into<AgentId>, profile_id: impl Into<ProfileId>) -> Self {
        Self {
            agent_id: agent_id.into(),
            profile_id: profile_id.into(),
        }
    }
}

/// Returns a stable lexical path identity without requiring the path to exist.
/// Discovery must remain deterministic for fixtures and must not create files.
fn normalize_root_path(path: &str) -> String {
    let mut normalized = Path::new(path).to_string_lossy().into_owned();
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

pub(crate) fn opaque_contract_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{prefix}:sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod v1_contract_tests {
    use super::*;

    #[test]
    fn typed_ids_round_trip_as_transparent_strings() {
        let ids = serde_json::json!({
            "agentId": AgentId::from("codex"),
            "installationId": InstallationId::from("codex:default"),
            "profileId": ProfileId::from("default"),
            "planId": PlanId::from("plan-1"),
            "receiptId": ReceiptId::from("receipt-1"),
            "workspaceKey": WorkspaceKey::from("workspace:sha256:test"),
        });

        assert_eq!(ids["agentId"], "codex");
        assert_eq!(ids["installationId"], "codex:default");
        assert_eq!(ids["profileId"], "default");
        assert_eq!(ids["planId"], "plan-1");
        assert_eq!(ids["receiptId"], "receipt-1");
        assert_eq!(ids["workspaceKey"], "workspace:sha256:test");
    }

    #[test]
    fn agent_context_round_trips_optional_project_scope() {
        let context = AgentContext {
            installation_id: InstallationId::from("codex:default"),
            project_path: Some("/Users/test/project".into()),
        };

        let json = serde_json::to_value(&context).unwrap();
        assert_eq!(json["installationId"], "codex:default");
        assert_eq!(json["projectPath"], "/Users/test/project");
        assert_eq!(
            serde_json::from_value::<AgentContext>(json).unwrap(),
            context
        );
    }

    #[test]
    fn agent_definition_carries_adapter_contract_version() {
        let definition = AgentDefinition {
            id: AgentId::from("claude-code"),
            display_name: "Claude Code".into(),
            adapter_version: 1,
        };

        assert_eq!(definition.adapter_version, 1);
        assert_eq!(definition.id.as_str(), "claude-code");
    }

    #[test]
    fn discovered_installation_exposes_an_installation_id() {
        let installation =
            AgentInstallation::with_id("codex:/Users/test/.codex", "codex", "/Users/test/.codex");

        let json = serde_json::to_value(&installation).unwrap();
        assert_eq!(json["id"], installation.id.as_str());
        assert_eq!(json["agentId"], "codex");
        assert_eq!(json["rootPath"], "/Users/test/.codex");
    }
}

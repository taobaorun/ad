use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
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
    pub agent_id: AgentId,
    pub root_path: String,
}

impl AgentInstallation {
    pub fn new(agent_id: impl Into<AgentId>, root_path: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            root_path: normalize_root_path(&root_path.into()),
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

/// Keeps the first installation for each Agent + normalized root identity.
pub fn deduplicate_installations(
    installations: impl IntoIterator<Item = AgentInstallation>,
) -> Vec<AgentInstallation> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for installation in installations {
        let key = (installation.agent_id.clone(), installation.root_path.clone());
        if seen.insert(key) {
            result.push(installation);
        }
    }
    result
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
        });

        assert_eq!(ids["agentId"], "codex");
        assert_eq!(ids["installationId"], "codex:default");
        assert_eq!(ids["profileId"], "default");
        assert_eq!(ids["planId"], "plan-1");
        assert_eq!(ids["receiptId"], "receipt-1");
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
        assert_eq!(serde_json::from_value::<AgentContext>(json).unwrap(), context);
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
}

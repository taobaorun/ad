use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Stable identifier for an Agent adapter.
pub type AgentId = String;

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
    pub profile_id: String,
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
    pub fn new(agent_id: impl Into<AgentId>, profile_id: impl Into<String>) -> Self {
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

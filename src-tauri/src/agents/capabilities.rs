use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AgentContext, AgentError, InstallationId, MutationPlan, ResourceRef, ResourceScope,
    ResourceSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Settings,
    Skills,
    Plugins,
    ProcessDetection,
    TerminalLaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityOperation {
    Inspect,
    Edit,
    Preview,
    Apply,
    Rollback,
    List,
    Install,
    Enable,
    Disable,
    Detect,
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityLimitation {
    pub code: String,
    pub message_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDescriptor {
    pub kind: CapabilityKind,
    #[serde(default)]
    pub scopes: BTreeSet<ResourceScope>,
    #[serde(default)]
    pub operations: BTreeSet<CapabilityOperation>,
    pub availability: CapabilityAvailability,
    #[serde(default)]
    pub limitations: Vec<CapabilityLimitation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEdit {
    pub resource: ResourceRef,
    pub media_type: String,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInstallRequest {
    pub logical_id: String,
    pub source: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessObservation {
    pub pid: u32,
    pub installation_id: InstallationId,
    pub executable: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecipe {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cwd: String,
}

macro_rules! descriptor_fields {
    () => {
        fn scopes(&self) -> BTreeSet<ResourceScope>;
        fn operations(&self) -> BTreeSet<CapabilityOperation>;
        fn availability(&self) -> CapabilityAvailability;

        fn limitations(&self) -> Vec<CapabilityLimitation> {
            Vec::new()
        }
    };
}

pub trait SettingsPort: Send + Sync {
    descriptor_fields!();

    fn inspect(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError>;
    fn plan_edit(
        &self,
        context: &AgentContext,
        edit: SettingsEdit,
    ) -> Result<MutationPlan, AgentError>;
}

pub trait SkillsPort: Send + Sync {
    descriptor_fields!();

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError>;
    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError>;
    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError>;
}

pub trait PluginsPort: Send + Sync {
    descriptor_fields!();

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError>;
    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError>;
    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError>;
}

pub trait ProcessPort: Send + Sync {
    descriptor_fields!();

    fn detect(&self, context: &AgentContext) -> Result<Vec<ProcessObservation>, AgentError>;
}

pub trait LaunchPort: Send + Sync {
    descriptor_fields!();

    fn recipe(&self, context: &AgentContext) -> Result<LaunchRecipe, AgentError>;
}

macro_rules! descriptor_from_port {
    ($function:ident, $port:ident, $kind:expr) => {
        pub(crate) fn $function(port: &dyn $port) -> CapabilityDescriptor {
            CapabilityDescriptor {
                kind: $kind,
                scopes: port.scopes(),
                operations: port.operations(),
                availability: port.availability(),
                limitations: port.limitations(),
            }
        }
    };
}

descriptor_from_port!(settings_descriptor, SettingsPort, CapabilityKind::Settings);
descriptor_from_port!(skills_descriptor, SkillsPort, CapabilityKind::Skills);
descriptor_from_port!(plugins_descriptor, PluginsPort, CapabilityKind::Plugins);
descriptor_from_port!(
    process_descriptor,
    ProcessPort,
    CapabilityKind::ProcessDetection
);
descriptor_from_port!(
    launch_descriptor,
    LaunchPort,
    CapabilityKind::TerminalLaunch
);

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn capability_descriptor_round_trips_scope_operations_and_limitations() {
        let descriptor = CapabilityDescriptor {
            kind: CapabilityKind::Settings,
            scopes: BTreeSet::from([ResourceScope::User, ResourceScope::Project]),
            operations: BTreeSet::from([
                CapabilityOperation::Inspect,
                CapabilityOperation::Preview,
                CapabilityOperation::Apply,
                CapabilityOperation::Rollback,
            ]),
            availability: CapabilityAvailability::Degraded,
            limitations: vec![CapabilityLimitation {
                code: "project_write_pending".into(),
                message_key: "agents.capabilities.projectWritePending".into(),
            }],
        };

        let json = serde_json::to_value(&descriptor).unwrap();
        assert_eq!(json["kind"], "settings");
        assert_eq!(json["availability"], "degraded");
        assert_eq!(
            json["limitations"][0]["messageKey"],
            "agents.capabilities.projectWritePending"
        );
        assert_eq!(
            serde_json::from_value::<CapabilityDescriptor>(json).unwrap(),
            descriptor
        );
    }
}

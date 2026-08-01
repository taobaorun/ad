use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    opaque_contract_id, AgentContext, AgentId, DeclarationKey, MutationKind, MutationPlan,
    OperationReceipt, PhysicalTargetId, PlanId, ResourceKey, ResourceKind, ResourceRef,
    ResourceScope, RiskFingerprint, WorkspaceKey,
};

impl ResourceKey {
    pub fn for_collection(
        workspace_key: &WorkspaceKey,
        agent_id: &AgentId,
        kind: ResourceKind,
        logical_id: &str,
        source_id: &str,
    ) -> Self {
        Self::from(opaque_contract_id(
            "resource",
            &[
                workspace_key.as_str(),
                agent_id.as_str(),
                kind.contract_name(),
                logical_id,
                source_id,
            ],
        ))
    }

    fn for_plan_resource(agent_id: &AgentId, resource: &ResourceRef) -> Self {
        Self::from(opaque_contract_id(
            "resource",
            &[
                agent_id.as_str(),
                resource.installation_id.as_str(),
                resource.project_path.as_deref().unwrap_or("user"),
                resource.kind.contract_name(),
                resource.logical_id.as_str(),
            ],
        ))
    }
}

impl DeclarationKey {
    pub fn for_layer(resource_key: &ResourceKey, layer: ResourceLayer, source_id: &str) -> Self {
        Self::from(opaque_contract_id(
            "declaration",
            &[resource_key.as_str(), layer.contract_name(), source_id],
        ))
    }
}

impl PhysicalTargetId {
    pub fn for_resource(resource: &ResourceRef) -> Self {
        Self::from(opaque_contract_id(
            "target",
            &[
                resource.installation_id.as_str(),
                resource.project_path.as_deref().unwrap_or("user"),
                resource.kind.contract_name(),
                match resource.scope {
                    ResourceScope::User => "user",
                    ResourceScope::Project => "project",
                },
                resource.logical_id.as_str(),
            ],
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLayer {
    System,
    User,
    Project,
    Runtime,
}

impl ResourceLayer {
    fn contract_name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Project => "project",
            Self::Runtime => "runtime",
        }
    }
}

/// Frontend-safe summary of one planned resource change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationPlanChangeView {
    pub resource: ResourceRef,
    pub kind: MutationKind,
    pub target: MutationTargetView,
    pub scope: ResourceScope,
    #[serde(default)]
    pub dependencies: Vec<ResourceKey>,
    #[serde(default)]
    pub activation_impact: Vec<ActivationImpactView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicMutationTargetKind {
    AgentResource,
    AdState,
}

/// Sanitized view of a backend-sealed mutation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationTargetView {
    pub id: PhysicalTargetId,
    pub kind: PublicMutationTargetKind,
    pub display: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationImpactKind {
    Configuration,
    Instructions,
    CodeExecution,
    Permissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationImpactView {
    pub kind: ActivationImpactKind,
    pub summary_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAcknowledgementCode {
    ConversionApply,
    DangerousPermissionExpansion,
    RollbackApply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanRiskLevel {
    Confirmation,
    Dangerous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcknowledgementRequirement {
    pub code: PlanAcknowledgementCode,
    pub risk: PlanRiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAcknowledgement {
    pub code: PlanAcknowledgementCode,
    pub accepted: bool,
}

/// Public view of a backend-owned plan. Mutation content and preconditions stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationPlanView {
    pub id: PlanId,
    pub agent_id: AgentId,
    pub context: AgentContext,
    #[serde(default)]
    pub changes: Vec<MutationPlanChangeView>,
    #[serde(default)]
    pub required_acknowledgements: Vec<AcknowledgementRequirement>,
    pub risk_fingerprint: RiskFingerprint,
    pub expires_at: DateTime<Utc>,
}

impl From<&MutationPlan> for MutationPlanView {
    fn from(plan: &MutationPlan) -> Self {
        Self {
            id: plan.id.clone(),
            agent_id: plan.agent_id.clone(),
            context: plan.context.clone(),
            changes: plan
                .mutations
                .iter()
                .map(|mutation| MutationPlanChangeView {
                    resource: mutation.resource.clone(),
                    kind: mutation.kind,
                    target: MutationTargetView {
                        id: PhysicalTargetId::for_resource(&mutation.resource),
                        kind: PublicMutationTargetKind::AgentResource,
                        display: format!(
                            "{}/{}",
                            mutation.resource.kind.contract_name(),
                            mutation.resource.logical_id
                        ),
                    },
                    scope: mutation.resource.scope,
                    dependencies: plan
                        .read_set
                        .iter()
                        .filter(|dependency| dependency.resource != mutation.resource)
                        .map(|dependency| {
                            ResourceKey::for_plan_resource(&plan.agent_id, &dependency.resource)
                        })
                        .collect(),
                    activation_impact: vec![activation_impact_for(mutation.resource.kind)],
                })
                .collect(),
            required_acknowledgements: Vec::new(),
            risk_fingerprint: risk_fingerprint(plan),
            expires_at: plan.expires_at,
        }
    }
}

fn activation_impact_for(kind: ResourceKind) -> ActivationImpactView {
    let (kind, summary_key) = match kind {
        ResourceKind::Settings => (
            ActivationImpactKind::Configuration,
            "agents.plan.impact.configuration",
        ),
        ResourceKind::Instructions | ResourceKind::Rules | ResourceKind::Agents => (
            ActivationImpactKind::Instructions,
            "agents.plan.impact.instructions",
        ),
        ResourceKind::Skills | ResourceKind::Plugins | ResourceKind::Hooks | ResourceKind::Mcp => (
            ActivationImpactKind::CodeExecution,
            "agents.plan.impact.codeExecution",
        ),
    };
    ActivationImpactView {
        kind,
        summary_key: summary_key.into(),
    }
}

fn risk_fingerprint(plan: &MutationPlan) -> RiskFingerprint {
    let mut parts = vec![
        plan.agent_id.as_str(),
        plan.context.installation_id.as_str(),
    ];
    for mutation in &plan.mutations {
        parts.push(mutation.resource.logical_id.as_str());
        parts.push(mutation.resource.kind.contract_name());
        parts.push(match mutation.kind {
            MutationKind::Create => "create",
            MutationKind::Replace => "replace",
            MutationKind::Delete => "delete",
        });
    }
    RiskFingerprint::from(opaque_contract_id("risk", &parts))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOperationOutcome {
    Changed,
    NoChange,
    External,
    Unsupported,
    Conflict,
    PartialFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOperationIssue {
    pub code: String,
    pub message_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<ResourceKey>,
}

/// Domain outcome around zero or one execution receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOperationReport {
    pub workspace_key: WorkspaceKey,
    pub outcome: WorkspaceOperationOutcome,
    #[serde(default)]
    pub issues: Vec<WorkspaceOperationIssue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<OperationReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionItemFinalState {
    Exact,
    Mapped,
    Unchanged,
    RequiresInput,
    Unsupported,
    Conflict,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionItemReport {
    pub item_id: String,
    pub state: ConversionItemFinalState,
    #[serde(default)]
    pub residuals: Vec<WorkspaceOperationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    pub workspace_key: WorkspaceKey,
    pub outcome: WorkspaceOperationOutcome,
    #[serde(default)]
    pub items: Vec<ConversionItemReport>,
    #[serde(default)]
    pub residuals: Vec<WorkspaceOperationIssue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<OperationReceipt>,
}

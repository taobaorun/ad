use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::execution_state::ExecutionState;
use super::skill_artifact_tree::{copy_tree_verified, inspect_tree, ArtifactLimits, TreeEntryKind};
use super::{
    opaque_contract_id, resolve_catalog_resource, AcknowledgementRequirement, ActivationImpactKind,
    ActivationImpactView, AgentContext, AgentError, AgentErrorCode, AgentId, ContentDigest,
    InstallationId, InventoryRevision, MutationKind, MutationPlanChangeView, MutationPlanView,
    MutationTargetView, OwnershipRecordId, PhysicalTargetId, PlanAcknowledgementCode, PlanId,
    PlanRiskLevel, PublicMutationTargetKind, ResourceAction, ResourceKey, ResourceKind,
    ResourceRef, ResourceScope, RiskFingerprint, UserWorkspaceDescriptor, WorkspaceKey,
    WorkspaceOperationOutcome, WorkspaceOperationReport,
};

const USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 1;
const COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserPluginManagementRecord {
    pub schema_version: u32,
    pub id: OwnershipRecordId,
    pub workspace_key: WorkspaceKey,
    pub installation_id: InstallationId,
    pub agent_id: AgentId,
    pub resource_id: String,
    pub source_id: String,
    pub install_id: String,
    pub native_id: String,
    pub marketplace_name: String,
    pub marketplace_root: String,
    pub artifact_path: String,
    pub artifact_digest: ContentDigest,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativePluginAction {
    Install,
    Enable,
    Disable,
    Remove,
}

impl NativePluginAction {
    fn from_resource_action(action: ResourceAction) -> Option<Self> {
        match action {
            ResourceAction::Install => Some(Self::Install),
            ResourceAction::Enable => Some(Self::Enable),
            ResourceAction::Disable => Some(Self::Disable),
            ResourceAction::Remove => Some(Self::Remove),
            _ => None,
        }
    }

    fn mutation_kind(self) -> MutationKind {
        match self {
            Self::Install => MutationKind::Create,
            Self::Enable | Self::Disable => MutationKind::Replace,
            Self::Remove => MutationKind::Delete,
        }
    }
}

#[derive(Clone)]
struct StoredUserPluginPlan {
    view: MutationPlanView,
    workspace_key: WorkspaceKey,
    inventory_revision: InventoryRevision,
    action: NativePluginAction,
    record: UserPluginManagementRecord,
    root_path: String,
    record_preexisting: bool,
}

#[derive(Default)]
pub struct UserPluginPlanStore {
    plans: Mutex<HashMap<PlanId, StoredUserPluginPlan>>,
    execution: tokio::sync::Mutex<()>,
}

impl UserPluginPlanStore {
    pub fn contains(&self, plan_id: &PlanId) -> bool {
        self.plans
            .lock()
            .is_ok_and(|plans| plans.contains_key(plan_id))
    }

    fn insert(
        &self,
        workspace: &UserWorkspaceDescriptor,
        inventory_revision: InventoryRevision,
        resource_key: ResourceKey,
        action: ResourceAction,
        record: UserPluginManagementRecord,
        record_preexisting: bool,
    ) -> Result<MutationPlanView, AgentError> {
        let action = NativePluginAction::from_resource_action(action).ok_or_else(|| {
            plugin_error(
                workspace,
                AgentErrorCode::Unsupported,
                "Unsupported user Plugin action",
            )
        })?;
        let context = AgentContext {
            installation_id: workspace.installation_id.clone(),
            project_path: None,
        };
        let resource = ResourceRef {
            installation_id: workspace.installation_id.clone(),
            project_path: None,
            kind: ResourceKind::Plugins,
            scope: ResourceScope::User,
            logical_id: format!("{}/{}", record.source_id, record.install_id),
        };
        let id = PlanId::from(uuid::Uuid::new_v4().to_string());
        let expires_at = Utc::now() + Duration::minutes(5);
        let risk_fingerprint = RiskFingerprint::from(opaque_contract_id(
            "user-plugin-risk",
            &[
                id.as_str(),
                workspace.key.as_str(),
                resource_key.as_str(),
                record.native_id.as_str(),
                match action {
                    NativePluginAction::Install => "install",
                    NativePluginAction::Enable => "enable",
                    NativePluginAction::Disable => "disable",
                    NativePluginAction::Remove => "remove",
                },
            ],
        ));
        let view = MutationPlanView {
            direct_apply_eligible: false,
            changed_settings_keys: Vec::new(),
            id: id.clone(),
            agent_id: workspace.agent_id.clone(),
            context,
            changes: vec![MutationPlanChangeView {
                resource: resource.clone(),
                kind: action.mutation_kind(),
                target: MutationTargetView {
                    id: PhysicalTargetId::for_resource(&resource),
                    kind: PublicMutationTargetKind::AgentResource,
                    display: format!("plugins/{} (user)", record.native_id),
                },
                scope: ResourceScope::User,
                dependencies: Vec::new(),
                activation_impact: vec![ActivationImpactView {
                    kind: ActivationImpactKind::CodeExecution,
                    summary_key: "agents.plan.impact.codeExecution".into(),
                }],
            }],
            required_acknowledgements: vec![AcknowledgementRequirement {
                code: PlanAcknowledgementCode::UserCollectionApply,
                risk: PlanRiskLevel::Confirmation,
            }],
            risk_fingerprint,
            expires_at,
        };
        let stored = StoredUserPluginPlan {
            view: view.clone(),
            workspace_key: workspace.key.clone(),
            inventory_revision,
            action,
            record,
            root_path: workspace.root_path.clone(),
            record_preexisting,
        };
        let mut plans = self
            .plans
            .lock()
            .map_err(|_| plugin_error(workspace, AgentErrorCode::Io, "Plugin plan store locked"))?;
        plans.retain(|_, plan| plan.view.expires_at > Utc::now());
        plans.insert(id, stored);
        Ok(view)
    }

    fn claim(
        &self,
        plan_id: &PlanId,
        expected_context: &AgentContext,
        expected_risk: &RiskFingerprint,
    ) -> Result<StoredUserPluginPlan, AgentError> {
        let mut plans = self.plans.lock().map_err(|_| AgentError {
            code: AgentErrorCode::Io,
            message: "Plugin plan store lock is unavailable".into(),
            agent_id: None,
            installation_id: Some(expected_context.installation_id.clone()),
            resource: None,
            retryable: true,
            details: Some(serde_json::json!({"phase": "user_plugin_plan"})),
        })?;
        let plan = plans.remove(plan_id).ok_or_else(|| AgentError {
            code: AgentErrorCode::InvalidPlan,
            message: "Unknown user Plugin plan".into(),
            agent_id: None,
            installation_id: Some(expected_context.installation_id.clone()),
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "user_plugin_plan"})),
        })?;
        if plan.view.expires_at <= Utc::now() {
            return Err(plan_error(
                &plan,
                AgentErrorCode::PlanExpired,
                "User Plugin plan expired",
            ));
        }
        if &plan.view.context != expected_context
            || &plan.view.risk_fingerprint != expected_risk
            || expected_context.project_path.is_some()
        {
            return Err(plan_error(
                &plan,
                AgentErrorCode::InvalidPlan,
                "User Plugin plan binding changed",
            ));
        }
        Ok(plan)
    }
}

pub fn list_user_plugin_management_records(
    workspace: &UserWorkspaceDescriptor,
) -> Result<Vec<UserPluginManagementRecord>, AgentError> {
    let state = ExecutionState::open()
        .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?;
    let mut records = Vec::new();
    for name in state
        .user_plugin_management()
        .entry_names()
        .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?
    {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let bytes = state
            .user_plugin_management()
            .read(name)
            .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?;
        let Ok(record) = serde_json::from_slice::<UserPluginManagementRecord>(&bytes) else {
            continue;
        };
        if validate_record(workspace, &record).is_ok() {
            records.push(record);
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

pub fn list_all_user_plugin_management_records(
) -> Result<Vec<UserPluginManagementRecord>, AgentError> {
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let mut records = Vec::new();
    for name in state
        .user_plugin_management()
        .entry_names()
        .map_err(global_plugin_error)?
    {
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        let bytes = state
            .user_plugin_management()
            .read(name)
            .map_err(global_plugin_error)?;
        if let Ok(record) = serde_json::from_slice::<UserPluginManagementRecord>(&bytes) {
            if record.schema_version == USER_PLUGIN_RECORD_SCHEMA_VERSION {
                records.push(record);
            }
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

pub fn user_plugin_record_for_resource(
    workspace: &UserWorkspaceDescriptor,
    resource_id: &str,
) -> Result<Option<UserPluginManagementRecord>, AgentError> {
    Ok(list_user_plugin_management_records(workspace)?
        .into_iter()
        .find(|record| record.resource_id == resource_id))
}

pub fn user_plugin_record_by_id(
    workspace: &UserWorkspaceDescriptor,
    record_id: &OwnershipRecordId,
) -> Result<Option<UserPluginManagementRecord>, AgentError> {
    Ok(list_user_plugin_management_records(workspace)?
        .into_iter()
        .find(|record| &record.id == record_id))
}

pub fn preview_user_plugin_action(
    workspace: &UserWorkspaceDescriptor,
    inventory_revision: InventoryRevision,
    resource_key: ResourceKey,
    resource_id: &str,
    action: ResourceAction,
    plans: &UserPluginPlanStore,
) -> Result<MutationPlanView, AgentError> {
    let existing = user_plugin_record_for_resource(workspace, resource_id)?;
    let record_preexisting = existing.is_some();
    let record = match action {
        ResourceAction::Install => match existing {
            Some(record) => record,
            None => proposed_user_plugin_record(workspace, resource_id)?,
        },
        ResourceAction::Enable | ResourceAction::Disable | ResourceAction::Remove => existing
            .ok_or_else(|| {
                plugin_error(
                    workspace,
                    AgentErrorCode::PermissionDenied,
                    "User Plugin is not proven as AD-managed",
                )
            })?,
        _ => {
            return Err(plugin_error(
                workspace,
                AgentErrorCode::Unsupported,
                "Unsupported user Plugin action",
            ))
        }
    };
    plans.insert(
        workspace,
        inventory_revision,
        resource_key,
        action,
        record,
        record_preexisting,
    )
}

pub async fn apply_user_plugin_plan(
    plan_id: &PlanId,
    expected_context: &AgentContext,
    expected_risk: &RiskFingerprint,
    confirmed: bool,
    plans: &UserPluginPlanStore,
) -> Result<WorkspaceOperationReport, AgentError> {
    if !confirmed {
        return Err(AgentError {
            code: AgentErrorCode::ConfirmationRequired,
            message: "User Plugin changes require confirmation".into(),
            agent_id: None,
            installation_id: Some(expected_context.installation_id.clone()),
            resource: None,
            retryable: false,
            details: Some(serde_json::json!({"phase": "user_plugin_apply"})),
        });
    }
    let _execution = plans.execution.lock().await;
    let plan = plans.claim(plan_id, expected_context, expected_risk)?;
    let current = super::inspect_user_resource_inventory(&plan.record.installation_id)?;
    if current.workspace.key != plan.workspace_key || current.revision != plan.inventory_revision {
        return Err(plan_error(
            &plan,
            AgentErrorCode::ResourceChanged,
            "User resource inventory changed after preview",
        ));
    }
    let runner = SystemPluginCommandRunner;
    execute_plan(&plan, &runner).await?;
    Ok(WorkspaceOperationReport {
        workspace_key: plan.workspace_key,
        outcome: WorkspaceOperationOutcome::Changed,
        issues: Vec::new(),
        receipt: None,
    })
}

#[derive(Debug)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[allow(async_fn_in_trait)]
trait PluginCommandRunner: Send + Sync {
    async fn run(
        &self,
        agent_id: &AgentId,
        root_path: &str,
        args: &[String],
    ) -> Result<CommandOutput, String>;
}

struct SystemPluginCommandRunner;

impl PluginCommandRunner for SystemPluginCommandRunner {
    async fn run(
        &self,
        agent_id: &AgentId,
        root_path: &str,
        args: &[String],
    ) -> Result<CommandOutput, String> {
        let executable_name = match agent_id.as_str() {
            "claude-code" => "claude",
            "codex" => "codex",
            _ => return Err("Unsupported Agent Plugin CLI".into()),
        };
        let executable = crate::terminal::resolve_bin(executable_name)
            .ok_or_else(|| format!("{executable_name} executable is unavailable"))?;
        let mut command = Command::new(executable);
        command.args(args).kill_on_drop(true);
        match agent_id.as_str() {
            "claude-code" => {
                command.env("CLAUDE_CONFIG_DIR", root_path);
            }
            "codex" => {
                command.env("CODEX_HOME", root_path);
            }
            _ => unreachable!(),
        }
        let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
            .await
            .map_err(|_| "Agent Plugin command timed out".to_string())?
            .map_err(|error| format!("Agent Plugin command failed to start: {error}"))?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: bounded_output(&output.stdout),
            stderr: bounded_output(&output.stderr),
        })
    }
}

async fn execute_plan<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
) -> Result<(), AgentError> {
    match plan.action {
        NativePluginAction::Install => {
            verify_plan_artifact(plan)?;
            prepare_marketplace(&plan.record).map_err(|error| {
                plan_error(
                    plan,
                    AgentErrorCode::Io,
                    format!("Failed to prepare Plugin marketplace: {error}"),
                )
            })?;
            if !plan.record_preexisting {
                write_record_new(&plan.record).map_err(|error| {
                    plan_error(
                        plan,
                        AgentErrorCode::Io,
                        format!("Failed to record Plugin ownership: {error}"),
                    )
                })?;
            }
            let add_marketplace = marketplace_add_args(&plan.record);
            if plan.record_preexisting {
                run_required_or_existing(plan, runner, &add_marketplace).await?;
            } else if let Err(error) = run_required(plan, runner, &add_marketplace).await {
                compensate_new_plugin_install(plan, runner, false).await;
                return Err(error);
            }
            let install = plugin_action_args(&plan.record, NativePluginAction::Install);
            if let Err(error) = run_required(plan, runner, &install).await {
                if !plan.record_preexisting {
                    compensate_new_plugin_install(plan, runner, true).await;
                }
                return Err(error);
            }
        }
        NativePluginAction::Enable | NativePluginAction::Disable => {
            let args = plugin_action_args(&plan.record, plan.action);
            run_required(plan, runner, &args).await?;
        }
        NativePluginAction::Remove => {
            let args = plugin_action_args(&plan.record, NativePluginAction::Remove);
            run_required_or_missing(plan, runner, &args).await?;
            let remove_marketplace = marketplace_remove_args(&plan.record);
            run_required_or_missing(plan, runner, &remove_marketplace).await?;
            remove_record(&plan.record.id).map_err(|error| {
                plan_error(
                    plan,
                    AgentErrorCode::Io,
                    format!("Failed to clear Plugin ownership: {error}"),
                )
            })?;
        }
    }
    Ok(())
}

async fn compensate_new_plugin_install<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
    uninstall_plugin: bool,
) {
    let plugin_removed = if uninstall_plugin {
        let uninstall = plugin_action_args(&plan.record, NativePluginAction::Remove);
        run_required_or_missing(plan, runner, &uninstall)
            .await
            .is_ok()
    } else {
        true
    };
    let remove_marketplace = marketplace_remove_args(&plan.record);
    let marketplace_removed = run_required_or_missing(plan, runner, &remove_marketplace)
        .await
        .is_ok();
    if plugin_removed && marketplace_removed {
        let _ = remove_record(&plan.record.id);
    }
}

fn verify_plan_artifact(plan: &StoredUserPluginPlan) -> Result<(), AgentError> {
    let state = ExecutionState::open().map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::Io,
            format!("Failed to open Plugin artifact state: {error}"),
        )
    })?;
    let artifact = Path::new(&plan.record.artifact_path);
    let valid_location = artifact.is_absolute()
        && artifact.parent() == Some(state.user_plugin_artifacts().display_path())
        && artifact.file_name().and_then(|name| name.to_str())
            == plan.record.artifact_digest.as_str().strip_prefix("sha256:");
    let valid_digest = inspect_tree(artifact, ArtifactLimits::default())
        .and_then(|manifest| manifest.digest())
        .is_ok_and(|digest| digest == plan.record.artifact_digest);
    if valid_location && valid_digest {
        Ok(())
    } else {
        Err(plan_error(
            plan,
            AgentErrorCode::ResourceChanged,
            "Confirmed user Plugin artifact changed before installation",
        ))
    }
}

async fn run_required<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
    args: &[String],
) -> Result<(), AgentError> {
    let output = runner
        .run(&plan.view.agent_id, &plan.root_path, args)
        .await
        .map_err(|error| plan_error(plan, AgentErrorCode::Io, error))?;
    if output.success {
        Ok(())
    } else {
        Err(plan_error(
            plan,
            AgentErrorCode::PartialFailure,
            command_failure(&output),
        ))
    }
}

async fn run_required_or_missing<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
    args: &[String],
) -> Result<(), AgentError> {
    let output = runner
        .run(&plan.view.agent_id, &plan.root_path, args)
        .await
        .map_err(|error| plan_error(plan, AgentErrorCode::Io, error))?;
    let combined = format!("{} {}", output.stdout, output.stderr).to_ascii_lowercase();
    if output.success || combined.contains("not installed") || combined.contains("not found") {
        Ok(())
    } else {
        Err(plan_error(
            plan,
            AgentErrorCode::PartialFailure,
            command_failure(&output),
        ))
    }
}

async fn run_required_or_existing<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
    args: &[String],
) -> Result<(), AgentError> {
    let output = runner
        .run(&plan.view.agent_id, &plan.root_path, args)
        .await
        .map_err(|error| plan_error(plan, AgentErrorCode::Io, error))?;
    let combined = format!("{} {}", output.stdout, output.stderr).to_ascii_lowercase();
    if output.success || combined.contains("already") {
        Ok(())
    } else {
        Err(plan_error(
            plan,
            AgentErrorCode::PartialFailure,
            command_failure(&output),
        ))
    }
}

fn marketplace_add_args(record: &UserPluginManagementRecord) -> Vec<String> {
    match record.agent_id.as_str() {
        "claude-code" => vec![
            "plugin".into(),
            "marketplace".into(),
            "add".into(),
            record.marketplace_root.clone(),
            "--scope".into(),
            "user".into(),
        ],
        "codex" => vec![
            "plugin".into(),
            "marketplace".into(),
            "add".into(),
            record.marketplace_root.clone(),
            "--json".into(),
        ],
        _ => Vec::new(),
    }
}

fn marketplace_remove_args(record: &UserPluginManagementRecord) -> Vec<String> {
    let mut args = vec![
        "plugin".into(),
        "marketplace".into(),
        "remove".into(),
        record.marketplace_name.clone(),
    ];
    if record.agent_id.as_str() == "claude-code" {
        args.extend(["--scope".into(), "user".into()]);
    }
    args
}

fn plugin_action_args(
    record: &UserPluginManagementRecord,
    action: NativePluginAction,
) -> Vec<String> {
    match (record.agent_id.as_str(), action) {
        ("claude-code", NativePluginAction::Install) => vec![
            "plugin".into(),
            "install".into(),
            record.native_id.clone(),
            "--scope".into(),
            "user".into(),
        ],
        ("claude-code", NativePluginAction::Enable) => vec![
            "plugin".into(),
            "enable".into(),
            record.native_id.clone(),
            "--scope".into(),
            "user".into(),
        ],
        ("claude-code", NativePluginAction::Disable) => vec![
            "plugin".into(),
            "disable".into(),
            record.native_id.clone(),
            "--scope".into(),
            "user".into(),
        ],
        ("claude-code", NativePluginAction::Remove) => vec![
            "plugin".into(),
            "uninstall".into(),
            record.native_id.clone(),
            "--scope".into(),
            "user".into(),
        ],
        ("codex", NativePluginAction::Install) => vec![
            "plugin".into(),
            "add".into(),
            record.native_id.clone(),
            "--json".into(),
        ],
        ("codex", NativePluginAction::Remove) => vec![
            "plugin".into(),
            "remove".into(),
            record.native_id.clone(),
            "--json".into(),
        ],
        _ => Vec::new(),
    }
}

pub(super) fn proposed_user_plugin_record(
    workspace: &UserWorkspaceDescriptor,
    resource_id: &str,
) -> Result<UserPluginManagementRecord, AgentError> {
    let resolved = resolve_user_plugin_candidate(workspace, resource_id)?;
    let plugin_name = plugin_name(workspace, &resolved.physical_path, &resolved.install_id)?;
    let marketplace_name = user_plugin_marketplace_name(workspace, resource_id);
    let state = ExecutionState::open()
        .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?;
    let marketplace_root = state
        .user_plugin_marketplaces()
        .display_path()
        .join(&marketplace_name);
    let (artifact_path, artifact_digest) = publish_user_plugin_artifact(&resolved.physical_path)
        .map_err(|error| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                error.to_string(),
            )
        })?;
    let id = OwnershipRecordId::from(opaque_contract_id(
        "user-plugin-management",
        &[workspace.key.as_str(), resource_id],
    ));
    let now = Utc::now();
    Ok(UserPluginManagementRecord {
        schema_version: USER_PLUGIN_RECORD_SCHEMA_VERSION,
        id,
        workspace_key: workspace.key.clone(),
        installation_id: workspace.installation_id.clone(),
        agent_id: workspace.agent_id.clone(),
        resource_id: resource_id.into(),
        source_id: resolved.source_id,
        install_id: resolved.install_id,
        native_id: format!("{plugin_name}@{marketplace_name}"),
        marketplace_name,
        marketplace_root: marketplace_root.to_string_lossy().into_owned(),
        artifact_path: artifact_path.to_string_lossy().into_owned(),
        artifact_digest,
        created_at: now,
        updated_at: now,
    })
}

pub(super) fn inspect_user_plugin_native_id(
    workspace: &UserWorkspaceDescriptor,
    resource_id: &str,
) -> Result<String, AgentError> {
    let resolved = resolve_user_plugin_candidate(workspace, resource_id)?;
    let plugin_name = plugin_name(workspace, &resolved.physical_path, &resolved.install_id)?;
    Ok(format!(
        "{plugin_name}@{}",
        user_plugin_marketplace_name(workspace, resource_id)
    ))
}

fn resolve_user_plugin_candidate(
    workspace: &UserWorkspaceDescriptor,
    resource_id: &str,
) -> Result<super::ResolvedCatalogResource, AgentError> {
    let resolved = resolve_catalog_resource(resource_id).map_err(|error| {
        plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            error.to_string(),
        )
    })?;
    if resolved.kind != ResourceKind::Plugins {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::InvalidPlan,
            "Catalog resource is not a Plugin",
        ));
    }
    Ok(resolved)
}

fn user_plugin_marketplace_name(workspace: &UserWorkspaceDescriptor, resource_id: &str) -> String {
    let marketplace_hash = opaque_contract_id(
        "user-plugin-marketplace",
        &[
            workspace.key.as_str(),
            resource_id,
            workspace.agent_id.as_str(),
        ],
    );
    let suffix = marketplace_hash
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(16)
        .collect::<String>();
    format!("ad-{suffix}")
}

fn publish_user_plugin_artifact(
    source: &Path,
) -> Result<(std::path::PathBuf, ContentDigest), String> {
    let limits = ArtifactLimits::default();
    let source_manifest =
        inspect_tree(source, limits).map_err(|error| format!("inspect source: {error}"))?;
    let state = ExecutionState::open().map_err(|error| format!("open state: {error}"))?;
    let temporary = state
        .user_plugin_artifacts()
        .display_path()
        .join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    if let Err(error) = copy_tree_verified(source, &temporary, &source_manifest, limits) {
        cleanup_plugin_artifact(&temporary);
        return Err(format!("copy snapshot: {error}"));
    }
    let digest = match source_manifest.digest() {
        Ok(digest) => digest,
        Err(error) => {
            cleanup_plugin_artifact(&temporary);
            return Err(format!("digest snapshot: {error}"));
        }
    };
    let directory_name = digest
        .as_str()
        .strip_prefix("sha256:")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "Invalid user Plugin artifact digest".to_string())?;
    let destination = state
        .user_plugin_artifacts()
        .display_path()
        .join(directory_name);
    if destination.exists() {
        let existing = inspect_tree(&destination, limits);
        cleanup_plugin_artifact(&temporary);
        if existing.is_ok_and(|existing| existing == source_manifest) {
            return Ok((destination, digest));
        }
        return Err("User Plugin artifact digest collision".into());
    }
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        if destination.exists()
            && inspect_tree(&destination, limits).is_ok_and(|existing| existing == source_manifest)
        {
            cleanup_plugin_artifact(&temporary);
            return Ok((destination, digest));
        }
        cleanup_plugin_artifact(&temporary);
        return Err(error.to_string());
    }
    if let Err(error) = make_plugin_artifact_read_only(&destination, &source_manifest) {
        cleanup_plugin_artifact(&destination);
        return Err(format!("seal snapshot: {error}"));
    }
    Ok((destination, digest))
}

fn make_plugin_artifact_read_only(
    root: &Path,
    manifest: &super::skill_artifact_tree::TreeManifest,
) -> Result<(), std::io::Error> {
    for entry in manifest.entries.iter().rev() {
        let path = root.join(&entry.path);
        let mode = match entry.kind {
            TreeEntryKind::Directory => 0o555,
            TreeEntryKind::File if entry.mode & 0o111 != 0 => 0o555,
            TreeEntryKind::File => 0o444,
            TreeEntryKind::Symlink => continue,
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o555))
}

fn cleanup_plugin_artifact(root: &Path) {
    if !root.exists() {
        return;
    }
    if let Ok(entries) = walk_plugin_artifact_directories(root) {
        for directory in entries {
            let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

fn walk_plugin_artifact_directories(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        for entry in std::fs::read_dir(&directories[index])? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                directories.push(entry.path());
            }
        }
        index += 1;
    }
    Ok(directories)
}

fn plugin_name(
    workspace: &UserWorkspaceDescriptor,
    root: &Path,
    expected: &str,
) -> Result<String, AgentError> {
    let descriptor = match workspace.agent_id.as_str() {
        "claude-code" => root.join(".claude-plugin/plugin.json"),
        "codex" => root.join(".codex-plugin/plugin.json"),
        _ => {
            return Err(plugin_error(
                workspace,
                AgentErrorCode::Unsupported,
                "Unsupported Agent Plugin format",
            ))
        }
    };
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&descriptor).map_err(|error| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                error.to_string(),
            )
        })?)
        .map_err(|error| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                error.to_string(),
            )
        })?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_segment(name))
        .ok_or_else(|| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                "Plugin descriptor has no valid name",
            )
        })?;
    if name != expected {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            "Plugin descriptor name differs from the catalog identity",
        ));
    }
    Ok(name.into())
}

fn prepare_marketplace(record: &UserPluginManagementRecord) -> std::io::Result<()> {
    let state = ExecutionState::open()?;
    let root_name = Path::new(&record.marketplace_root)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("Invalid user Plugin marketplace root"))?;
    let root = state
        .user_plugin_marketplaces()
        .open_or_create_directory(root_name)?;
    let plugins = root.open_or_create_directory("plugins")?;
    let plugin_link = plugins.display_path().join(&record.install_id);
    match std::fs::symlink_metadata(&plugin_link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(&plugin_link)? != Path::new(&record.artifact_path) {
                return Err(std::io::Error::other("Plugin marketplace source changed"));
            }
        }
        Ok(_) => {
            return Err(std::io::Error::other(
                "Plugin marketplace target is occupied",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&record.artifact_path, &plugin_link)?;
        }
        Err(error) => return Err(error),
    }
    let relative_source = format!("./plugins/{}", record.install_id);
    let manifest = if record.agent_id.as_str() == "claude-code" {
        let metadata = root.open_or_create_directory(".claude-plugin")?;
        (
            metadata,
            serde_json::json!({
                "name": record.marketplace_name,
                "owner": {"name": "AD"},
                "plugins": [{"name": record.install_id, "source": relative_source}]
            }),
        )
    } else {
        let agents = root.open_or_create_directory(".agents")?;
        let metadata = agents.open_or_create_directory("plugins")?;
        (
            metadata,
            serde_json::json!({
                "name": record.marketplace_name,
                "plugins": [{
                    "name": record.install_id,
                    "source": {"source": "local", "path": relative_source}
                }]
            }),
        )
    };
    manifest.0.write_atomic(
        "marketplace.json",
        &serde_json::to_vec_pretty(&manifest.1).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
fn write_record(record: &UserPluginManagementRecord) -> std::io::Result<()> {
    let state = ExecutionState::open()?;
    state.user_plugin_management().write_atomic(
        &record_file_name(&record.id),
        &serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?,
    )
}

fn write_record_new(record: &UserPluginManagementRecord) -> std::io::Result<()> {
    let state = ExecutionState::open()?;
    state.user_plugin_management().write_atomic_new(
        &record_file_name(&record.id),
        &serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
pub(super) fn persist_user_plugin_record_for_test(
    record: &UserPluginManagementRecord,
) -> std::io::Result<()> {
    write_record(record)
}

fn remove_record(id: &OwnershipRecordId) -> std::io::Result<()> {
    let state = ExecutionState::open()?;
    match state.user_plugin_management().remove(&record_file_name(id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate_record(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
) -> Result<(), AgentError> {
    let state = ExecutionState::open()
        .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?;
    let resolved = resolve_catalog_resource(&record.resource_id).map_err(|error| {
        plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            error.to_string(),
        )
    })?;
    let artifact_path = Path::new(&record.artifact_path);
    let artifact_digest = inspect_tree(artifact_path, ArtifactLimits::default())
        .and_then(|manifest| manifest.digest())
        .map_err(|error| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                error.to_string(),
            )
        })?;
    let valid = record.schema_version == USER_PLUGIN_RECORD_SCHEMA_VERSION
        && record.workspace_key == workspace.key
        && record.installation_id == workspace.installation_id
        && record.agent_id == workspace.agent_id
        && record.id
            == OwnershipRecordId::from(opaque_contract_id(
                "user-plugin-management",
                &[workspace.key.as_str(), &record.resource_id],
            ))
        && valid_segment(&record.install_id)
        && valid_segment(&record.marketplace_name)
        && record.native_id == format!("{}@{}", record.install_id, record.marketplace_name)
        && Path::new(&record.marketplace_root).is_absolute()
        && Path::new(&record.marketplace_root).parent()
            == Some(state.user_plugin_marketplaces().display_path())
        && artifact_path.is_absolute()
        && artifact_path.parent() == Some(state.user_plugin_artifacts().display_path())
        && artifact_path.file_name().and_then(|name| name.to_str())
            == record.artifact_digest.as_str().strip_prefix("sha256:")
        && record.source_id == resolved.source_id
        && record.install_id == resolved.install_id
        && record.artifact_digest == artifact_digest
        && plugin_name(workspace, artifact_path, &record.install_id).is_ok();
    if valid {
        Ok(())
    } else {
        Err(plugin_error(
            workspace,
            AgentErrorCode::PermissionDenied,
            "User Plugin management record is invalid",
        ))
    }
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn record_file_name(id: &OwnershipRecordId) -> String {
    format!("{}.json", id.as_str().replace(':', "_"))
}

fn bounded_output(bytes: &[u8]) -> String {
    const LIMIT: usize = 4096;
    String::from_utf8_lossy(&bytes[..bytes.len().min(LIMIT)])
        .trim()
        .to_owned()
}

fn command_failure(output: &CommandOutput) -> String {
    if !output.stderr.is_empty() {
        format!("Agent Plugin command failed: {}", output.stderr)
    } else if !output.stdout.is_empty() {
        format!("Agent Plugin command failed: {}", output.stdout)
    } else {
        "Agent Plugin command failed".into()
    }
}

fn plan_error(
    plan: &StoredUserPluginPlan,
    code: AgentErrorCode,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: Some(plan.view.agent_id.clone()),
        installation_id: Some(plan.view.context.installation_id.clone()),
        resource: plan
            .view
            .changes
            .first()
            .map(|change| change.resource.clone()),
        retryable: matches!(code, AgentErrorCode::Io | AgentErrorCode::ResourceChanged),
        details: Some(serde_json::json!({"phase": "user_plugin_apply"})),
    }
}

fn plugin_error(
    workspace: &UserWorkspaceDescriptor,
    code: AgentErrorCode,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.installation_id.clone()),
        resource: None,
        retryable: matches!(code, AgentErrorCode::Io | AgentErrorCode::ResourceChanged),
        details: Some(serde_json::json!({"phase": "user_plugin"})),
    }
}

fn global_plugin_error(error: impl ToString) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: error.to_string(),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: true,
        details: Some(serde_json::json!({"phase": "user_plugin"})),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::sync::Mutex;

    use super::*;
    struct HomeGuard(Option<OsString>);

    impl HomeGuard {
        fn set(root: &Path) -> Self {
            let previous = std::env::var_os("AD_HOME");
            std::env::set_var("AD_HOME", root);
            Self(previous)
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("AD_HOME", value),
                None => std::env::remove_var("AD_HOME"),
            }
        }
    }

    #[derive(Default)]
    struct FakeRunner {
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl PluginCommandRunner for FakeRunner {
        async fn run(
            &self,
            _agent_id: &AgentId,
            _root_path: &str,
            args: &[String],
        ) -> Result<CommandOutput, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            Ok(CommandOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    struct ScriptedRunner {
        calls: Mutex<Vec<Vec<String>>>,
        outputs: Mutex<VecDeque<CommandOutput>>,
    }

    impl PluginCommandRunner for ScriptedRunner {
        async fn run(
            &self,
            _agent_id: &AgentId,
            _root_path: &str,
            args: &[String],
        ) -> Result<CommandOutput, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "No scripted Plugin command output".to_string())
        }
    }

    fn command_output(success: bool, stderr: &str) -> CommandOutput {
        CommandOutput {
            success,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    fn stored_plan(
        root: &Path,
        record: UserPluginManagementRecord,
        action: NativePluginAction,
    ) -> StoredUserPluginPlan {
        let context = AgentContext {
            installation_id: record.installation_id.clone(),
            project_path: None,
        };
        StoredUserPluginPlan {
            view: MutationPlanView {
                direct_apply_eligible: false,
                changed_settings_keys: Vec::new(),
                id: PlanId::from("plan:test"),
                agent_id: record.agent_id.clone(),
                context,
                changes: Vec::new(),
                required_acknowledgements: Vec::new(),
                risk_fingerprint: RiskFingerprint::from("risk:test"),
                expires_at: Utc::now() + Duration::minutes(5),
            },
            workspace_key: record.workspace_key.clone(),
            inventory_revision: InventoryRevision::from("inventory:test"),
            action,
            record,
            root_path: root.to_string_lossy().into_owned(),
            record_preexisting: false,
        }
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn native_plugin_install_and_remove_are_sealed_and_recorded() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "plugin").unwrap();
        let (artifact, artifact_digest) = publish_user_plugin_artifact(&source).unwrap();
        let marketplace_root = temp
            .path()
            .join(".ad/state/user-plugin-marketplaces/ad-test");
        let record = UserPluginManagementRecord {
            schema_version: USER_PLUGIN_RECORD_SCHEMA_VERSION,
            id: OwnershipRecordId::from("ownership:user-plugin-test"),
            workspace_key: WorkspaceKey::from("workspace:user-test"),
            installation_id: InstallationId::from("claude-code:test"),
            agent_id: AgentId::from("claude-code"),
            resource_id: "catalog:test".into(),
            source_id: "source:test".into(),
            install_id: "demo".into(),
            native_id: "demo@ad-test".into(),
            marketplace_name: "ad-test".into(),
            marketplace_root: marketplace_root.to_string_lossy().into_owned(),
            artifact_path: artifact.to_string_lossy().into_owned(),
            artifact_digest,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let runner = FakeRunner::default();
        execute_plan(
            &stored_plan(temp.path(), record.clone(), NativePluginAction::Install),
            &runner,
        )
        .await
        .unwrap();

        assert!(marketplace_root
            .join(".claude-plugin/marketplace.json")
            .is_file());
        assert_eq!(
            std::fs::read_link(marketplace_root.join("plugins/demo")).unwrap(),
            artifact
        );
        assert_eq!(runner.calls.lock().unwrap().len(), 2);
        let state = ExecutionState::open().unwrap();
        assert!(state
            .user_plugin_management()
            .read(&record_file_name(&record.id))
            .is_ok());

        execute_plan(
            &stored_plan(temp.path(), record.clone(), NativePluginAction::Remove),
            &runner,
        )
        .await
        .unwrap();
        assert!(state
            .user_plugin_management()
            .read(&record_file_name(&record.id))
            .is_err());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0][..3], ["plugin", "marketplace", "add"]);
        assert_eq!(calls[1][..2], ["plugin", "install"]);
        assert_eq!(calls[2][..2], ["plugin", "uninstall"]);
        assert_eq!(
            calls[3],
            [
                "plugin",
                "marketplace",
                "remove",
                "ad-test",
                "--scope",
                "user"
            ]
        );
        cleanup_plugin_artifact(&artifact);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn plugin_preview_publishes_an_immutable_content_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "confirmed").unwrap();

        let (snapshot, first_digest) = publish_user_plugin_artifact(&source).unwrap();
        std::fs::write(source.join("README.md"), "changed later").unwrap();
        let (next_snapshot, next_digest) = publish_user_plugin_artifact(&source).unwrap();

        assert_eq!(
            std::fs::read_to_string(snapshot.join("README.md")).unwrap(),
            "confirmed"
        );
        assert_ne!(first_digest, next_digest);
        assert_ne!(snapshot, next_snapshot);
        cleanup_plugin_artifact(&snapshot);
        cleanup_plugin_artifact(&next_snapshot);
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn failed_install_keeps_management_record_when_compensation_fails() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "plugin").unwrap();
        let (artifact, artifact_digest) = publish_user_plugin_artifact(&source).unwrap();
        let record = UserPluginManagementRecord {
            schema_version: USER_PLUGIN_RECORD_SCHEMA_VERSION,
            id: OwnershipRecordId::from("ownership:user-plugin-compensation"),
            workspace_key: WorkspaceKey::from("workspace:user-compensation"),
            installation_id: InstallationId::from("claude-code:test"),
            agent_id: AgentId::from("claude-code"),
            resource_id: "catalog:test".into(),
            source_id: "source:test".into(),
            install_id: "demo".into(),
            native_id: "demo@ad-test".into(),
            marketplace_name: "ad-test".into(),
            marketplace_root: temp
                .path()
                .join(".ad/state/user-plugin-marketplaces/ad-test")
                .to_string_lossy()
                .into_owned(),
            artifact_path: artifact.to_string_lossy().into_owned(),
            artifact_digest,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let runner = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([
                command_output(true, ""),
                command_output(false, "install failed"),
                command_output(true, ""),
                command_output(false, "marketplace cleanup failed"),
            ])),
        };

        let error = execute_plan(
            &stored_plan(temp.path(), record.clone(), NativePluginAction::Install),
            &runner,
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::PartialFailure);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[2][..2], ["plugin", "uninstall"]);
        assert_eq!(calls[3][..3], ["plugin", "marketplace", "remove"]);
        drop(calls);
        let state = ExecutionState::open().unwrap();
        assert!(state
            .user_plugin_management()
            .read(&record_file_name(&record.id))
            .is_ok());
        remove_record(&record.id).unwrap();
        cleanup_plugin_artifact(&artifact);
    }
}

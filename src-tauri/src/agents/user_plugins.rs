use std::collections::HashMap;
use std::os::unix::fs::symlink;
#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::execution_confinement::ConfinedTarget;
use super::execution_fs::TargetState;
use super::execution_state::ExecutionState;
#[cfg(test)]
use super::skill_artifact_tree::{copy_tree_verified, TreeEntryKind};
use super::skill_artifact_tree::{inspect_tree, ArtifactLimits};
use super::{
    load_resource_catalog_snapshot, opaque_contract_id, resolve_catalog_resource,
    AcknowledgementRequirement, ActivationImpactKind, ActivationImpactView, AgentContext,
    AgentError, AgentErrorCode, AgentId, ContentDigest, InstallationId, InventoryRevision,
    MutationKind, MutationPlanChangeView, MutationPlanView, MutationTargetView, OwnershipRecordId,
    PhysicalTargetId, PlanAcknowledgementCode, PlanId, PlanRiskLevel, PublicMutationTargetKind,
    ResourceAction, ResourceKey, ResourceKind, ResourceLifecycle, ResourceRef, ResourceScope,
    ResourceTrackingPolicy, RiskFingerprint, UserWorkspaceDescriptor, WorkspaceKey,
    WorkspaceOperationOutcome, WorkspaceOperationReport,
};

const USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 6;
const PROJECTION_USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 5;
const REMOVAL_USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 4;
const RESOLVED_METADATA_USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 3;
const SOURCE_TRACKING_USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 2;
const LEGACY_USER_PLUGIN_RECORD_SCHEMA_VERSION: u32 = 1;
const COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(60);
const USER_PLUGIN_MIGRATION_JOURNAL_VERSION: u32 = 1;

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
    #[serde(default)]
    pub tracking_policy: ResourceTrackingPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_content_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_install_path: Option<String>,
    /// Names of the child links owned by AD in a physical Codex cache directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_entries: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal_phase: Option<UserPluginRemovalPhase>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserPluginRemovalPhase {
    LinkRemovalPending,
    LinkRemoved,
    NativeRemoved,
    MarketplaceRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserPluginMigrationJournal {
    schema_version: u32,
    record_id: OwnershipRecordId,
    target_path: String,
    source_path: String,
    backup_path: String,
    previous_digest: ContentDigest,
    next_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserPluginCacheRepairJournal {
    before: UserPluginManagementRecord,
    after: UserPluginManagementRecord,
    backup_name: Option<String>,
    original_digest: Option<ContentDigest>,
    original_link: Option<String>,
}

fn cache_repair_journal_name(record: &UserPluginManagementRecord) -> String {
    format!("cache-repair-{}.json", record.id.as_str().replace(':', "_"))
}

#[cfg(test)]
pub(super) fn simulate_published_cache_repair_for_test(
    workspace: &UserWorkspaceDescriptor,
    before: &UserPluginManagementRecord,
) -> UserPluginManagementRecord {
    let state = ExecutionState::open().unwrap();
    let resolved = resolve_catalog_resource(&before.resource_id).unwrap();
    let mut after = before.clone();
    after.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
    after.cache_digest =
        Some(super::execution_tree::tracked_source_digest(&resolved.stable_path).unwrap());
    after.linked_install_path = Some(
        Path::new(before.linked_install_path.as_deref().unwrap())
            .with_file_name(
                super::resource_version::read_resource_version(
                    &resolved.stable_path,
                    "",
                    ResourceKind::Plugins,
                )
                .unwrap(),
            )
            .to_string_lossy()
            .into_owned(),
    );
    let journal = UserPluginCacheRepairJournal {
        before: before.clone(),
        after: after.clone(),
        backup_name: None,
        original_digest: None,
        original_link: None,
    };
    state
        .source_tracking_migration_journals()
        .write_atomic_new(
            &cache_repair_journal_name(before),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
    record_cache_target(
        workspace,
        &after,
        Path::new(after.linked_install_path.as_deref().unwrap()),
    )
    .unwrap()
    .write_tracked_source_atomic(&resolved.stable_path, after.cache_digest.as_ref().unwrap())
    .unwrap();
    after
}

#[cfg(test)]
pub(super) fn simulate_cache_rename_interruption_for_test(
    workspace: &UserWorkspaceDescriptor,
    before: &UserPluginManagementRecord,
) -> PathBuf {
    let state = ExecutionState::open().unwrap();
    let path = Path::new(before.linked_install_path.as_deref().unwrap());
    let target = record_cache_target(workspace, before, path).unwrap();
    let original = target.observe().unwrap();
    let backup_name = format!("cache-repair-{}", uuid::Uuid::new_v4());
    let backup = matches!(original, TargetState::Directory(_));
    if backup {
        target
            .copy_directory_to(state.user_plugin_artifacts(), &backup_name)
            .unwrap();
    }
    let mut after = before.clone();
    after.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
    after.cache_digest = Some(
        super::execution_tree::tracked_source_digest(Path::new(
            before.linked_source_path.as_deref().unwrap(),
        ))
        .unwrap(),
    );
    let journal = UserPluginCacheRepairJournal {
        before: before.clone(),
        after,
        backup_name: backup.then_some(backup_name),
        original_digest: original.digest(),
        original_link: match original {
            TargetState::Symlink(path) => Some(path.to_string_lossy().into_owned()),
            _ => None,
        },
    };
    state
        .source_tracking_migration_journals()
        .write_atomic_new(
            &cache_repair_journal_name(before),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
    let previous = format!(".interrupted-previous-{}", uuid::Uuid::new_v4());
    target.rename_to_sibling(&previous).unwrap();
    path.with_file_name(previous)
}

fn recover_plugin_cache_repair(
    workspace: &UserWorkspaceDescriptor,
    state: &ExecutionState,
    record: &UserPluginManagementRecord,
) -> Result<Option<UserPluginManagementRecord>, AgentError> {
    let directory = state.source_tracking_migration_journals();
    let name = cache_repair_journal_name(record);
    let bytes = match directory.read(&name) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(global_plugin_error(error)),
    };
    let journal: UserPluginCacheRepairJournal =
        serde_json::from_slice(&bytes).map_err(global_plugin_error)?;
    if (record != &journal.before && record != &journal.after)
        || journal.before.id != journal.after.id
        || journal.before.resource_id != journal.after.resource_id
        || journal.before.native_id != journal.after.native_id
        || journal.before.workspace_key != journal.after.workspace_key
    {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::PermissionDenied,
            "Plugin cache repair journal no longer matches ownership",
        ));
    }
    validate_record_with_source_requirement(workspace, &journal.before, false)?;
    validate_record_with_source_requirement(workspace, &journal.after, false)?;
    let target = record_cache_target(
        workspace,
        &journal.after,
        Path::new(
            journal
                .after
                .linked_install_path
                .as_deref()
                .ok_or_else(|| global_plugin_error("Missing cache repair target"))?,
        ),
    )?;
    let published = matches!(target.observe()?, TargetState::Directory(ref digest) if Some(digest) == journal.after.cache_digest.as_ref());
    if published {
        write_record(&journal.after).map_err(global_plugin_error)?;
    } else if let Some(backup) = &journal.backup_name {
        if !backup.starts_with("cache-repair-") || Path::new(backup).components().count() != 1 {
            return Err(global_plugin_error("Invalid repair backup name"));
        }
        let observed = target.observe()?;
        if matches!(observed, TargetState::Missing) {
            let backup = state
                .user_plugin_artifacts()
                .open_directory(backup)
                .map_err(global_plugin_error)?;
            if Some(backup.digest().map_err(global_plugin_error)?) != journal.original_digest {
                return Err(global_plugin_error("Plugin repair backup changed"));
            }
            target.write_directory_from(&backup)?;
        } else if observed.digest() != journal.original_digest {
            return Err(plugin_error(
                workspace,
                AgentErrorCode::PermissionDenied,
                "Plugin cache changed during interrupted repair; backup retained",
            ));
        }
    } else if let Some(link) = &journal.original_link {
        let observed = target.observe()?;
        if matches!(observed, TargetState::Missing) {
            target.write_symlink_atomic(Path::new(link))?;
        } else if !matches!(observed, TargetState::Symlink(ref current) if current == Path::new(link))
        {
            return Err(global_plugin_error(
                "Plugin repair target changed; journal retained",
            ));
        }
    } else if !matches!(target.observe()?, TargetState::Missing) {
        return Err(global_plugin_error(
            "Plugin repair target changed; journal retained",
        ));
    }
    if !published {
        write_record(&journal.before).map_err(global_plugin_error)?;
    }
    let retired = if published {
        retire_previous_plugin_cache(workspace, &journal.before, &journal.after)?
    } else {
        None
    };
    let cleanup_backup = if let Some(backup) = &journal.backup_name {
        match state.user_plugin_artifacts().open_directory(backup) {
            Ok(stored) => {
                if Some(stored.digest().map_err(global_plugin_error)?) != journal.original_digest {
                    return Err(global_plugin_error(
                        "Plugin repair backup changed; backup retained",
                    ));
                }
                Some(backup)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(global_plugin_error(error)),
        }
    } else {
        None
    };
    // Once cache and record agree, retire the journal before best-effort backup cleanup.
    directory.remove(&name).map_err(global_plugin_error)?;
    if let Some(backup) = cleanup_backup {
        if let Err(error) = state.user_plugin_artifacts().remove(backup) {
            tracing::warn!(%error, "Plugin repair backup retained after recovery");
        }
    }
    if let Some(retired) = retired {
        match cache_matches_record(&journal.before, &retired) {
            Ok(true) => {
                if let Err(error) = retired.remove() {
                    tracing::warn!(%error, "retired Plugin cache retained after recovery");
                }
            }
            Ok(false) => tracing::warn!("changed retired Plugin cache retained after recovery"),
            Err(error) => tracing::warn!(%error, "retired Plugin cache retained after recovery"),
        }
    }
    Ok(Some(if published {
        journal.after
    } else {
        journal.before
    }))
}

fn cache_matches_record(
    record: &UserPluginManagementRecord,
    target: &ConfinedTarget,
) -> Result<bool, AgentError> {
    Ok(match target.observe()? {
        TargetState::Directory(ref digest) if record.cache_digest.as_ref() == Some(digest) => true,
        TargetState::Directory(_) => match (&record.projection_entries, &record.linked_source_path)
        {
            (Some(entries), Some(source)) => {
                target.source_links_match(Path::new(source), entries)?
            }
            _ => false,
        },
        TargetState::Symlink(ref link) => record
            .linked_source_path
            .as_deref()
            .is_some_and(|source| link == Path::new(source)),
        _ => false,
    })
}

fn cache_retirement_path(
    before: &UserPluginManagementRecord,
    after: &UserPluginManagementRecord,
) -> Option<PathBuf> {
    let installed = Path::new(before.linked_install_path.as_deref()?);
    if Some(installed) == after.linked_install_path.as_deref().map(Path::new) {
        return None;
    }
    let identity = format!(
        "{}:{}:{:?}",
        before.id, before.updated_at, after.cache_digest
    );
    Some(installed.with_file_name(format!(
            ".ad-retired-{}",
            ContentDigest::sha256(identity.as_bytes())
                .as_str()
                .trim_start_matches("sha256:")
        )))
}

#[cfg(test)]
pub(super) fn cache_retirement_path_for_test(
    before: &UserPluginManagementRecord,
    after: &UserPluginManagementRecord,
) -> PathBuf {
    cache_retirement_path(before, after).unwrap()
}

fn retire_previous_plugin_cache(
    workspace: &UserWorkspaceDescriptor,
    before: &UserPluginManagementRecord,
    after: &UserPluginManagementRecord,
) -> Result<Option<ConfinedTarget>, AgentError> {
    let Some(retired_path) = cache_retirement_path(before, after) else {
        return Ok(None);
    };
    let old_path = Path::new(before.linked_install_path.as_deref().unwrap());
    let old = record_cache_target(workspace, before, old_path)?;
    let retired = record_cache_target(workspace, before, &retired_path)?;
    if matches!(old.observe()?, TargetState::Missing) {
        return if cache_matches_record(before, &retired)? {
            Ok(Some(retired))
        } else {
            Ok(None)
        };
    }
    if cache_matches_record(before, &old)? {
        old.rename_to_sibling(retired_path.file_name().unwrap().to_str().unwrap())?;
        return Ok(Some(retired));
    }
    let old_version = old_path.file_name().and_then(|name| name.to_str());
    let new_version = after
        .linked_install_path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str());
    if matches!(old.observe()?, TargetState::Directory(_))
        && old_version
            .zip(new_version)
            .is_some_and(|(old, new)| super::codex_plugins::cache_version_is_newer(old, new))
    {
        return Err(global_plugin_error(
            "Previous higher Plugin cache changed during synchronization; journal retained",
        ));
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativePluginAction {
    Install,
    Repair,
    Enable,
    Disable,
    Remove,
}

impl NativePluginAction {
    fn from_resource_action(action: ResourceAction) -> Option<Self> {
        match action {
            ResourceAction::Install => Some(Self::Install),
            ResourceAction::Update => Some(Self::Repair),
            ResourceAction::Enable => Some(Self::Enable),
            ResourceAction::Disable => Some(Self::Disable),
            ResourceAction::Remove => Some(Self::Remove),
            _ => None,
        }
    }

    fn mutation_kind(self) -> MutationKind {
        match self {
            Self::Install => MutationKind::Create,
            Self::Enable | Self::Disable | Self::Repair => MutationKind::Replace,
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
                    NativePluginAction::Repair => "repair",
                    NativePluginAction::Enable => "enable",
                    NativePluginAction::Disable => "disable",
                    NativePluginAction::Remove => "remove",
                },
                "latest",
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
        let record = serde_json::from_slice::<UserPluginManagementRecord>(&bytes)
            .map_err(global_plugin_error)?;
        if !matches!(
            record.schema_version,
            LEGACY_USER_PLUGIN_RECORD_SCHEMA_VERSION
                | SOURCE_TRACKING_USER_PLUGIN_RECORD_SCHEMA_VERSION
                | RESOLVED_METADATA_USER_PLUGIN_RECORD_SCHEMA_VERSION
                | REMOVAL_USER_PLUGIN_RECORD_SCHEMA_VERSION
                | PROJECTION_USER_PLUGIN_RECORD_SCHEMA_VERSION
                | USER_PLUGIN_RECORD_SCHEMA_VERSION
        ) {
            return Err(global_plugin_error(format!(
                "Unsupported user Plugin record schema {}",
                record.schema_version
            )));
        }
        records.push(record);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

pub fn migrate_legacy_user_plugin_source_tracking() -> Result<usize, AgentError> {
    let migration_state = ExecutionState::open().map_err(global_plugin_error)?;
    let mut migrated = 0;
    for legacy in list_all_user_plugin_management_records()? {
        let workspace = match super::resolve_user_agent_workspace(&legacy.installation_id) {
            Ok(workspace) => workspace,
            Err(_) => continue,
        };
        if workspace.agent_id.as_str() == "codex" {
            // Startup recovers an authorized interrupted update, but never syncs new source bytes.
            let cache_root = Path::new(&workspace.root_path)
                .join("plugins/cache")
                .join(&legacy.marketplace_name)
                .join(&legacy.install_id);
            let _lock = super::TargetLockSet::acquire_for_ad_states(
                &[
                    cache_root,
                    migration_state
                        .user_plugin_management()
                        .display_path()
                        .join(record_file_name(&legacy.id)),
                    super::resource_lifecycle_lock_target(&migration_state, &legacy.resource_id),
                ],
                &format!("plugin-update-recovery:{}", uuid::Uuid::new_v4()),
                &migration_state,
            )
            .map_err(global_plugin_error)?;
            recover_previous_codex_migration(&workspace, &migration_state, &legacy)?;
            recover_plugin_cache_repair(&workspace, &migration_state, &legacy)?;
            continue;
        }
        if let (Some(source), Some(installed)) = (
            legacy.linked_source_path.as_deref(),
            legacy.linked_install_path.as_deref(),
        ) {
            if std::fs::read_link(installed)
                .ok()
                .is_some_and(|link| link == Path::new(source))
            {
                let operation_id = format!("user-plugin-source-recovery:{}", uuid::Uuid::new_v4());
                let _lock = super::TargetLockSet::acquire_for_ad_states(
                    &[
                        Path::new(installed).to_path_buf(),
                        migration_state
                            .user_plugin_management()
                            .display_path()
                            .join(record_file_name(&legacy.id)),
                        super::resource_lifecycle_lock_target(
                            &migration_state,
                            &legacy.resource_id,
                        ),
                    ],
                    &operation_id,
                    &migration_state,
                )
                .map_err(|error| plugin_error(&workspace, AgentErrorCode::Io, error.to_string()))?;
                let still_current = migration_state
                    .user_plugin_management()
                    .read(&record_file_name(&legacy.id))
                    .ok()
                    .and_then(|bytes| {
                        serde_json::from_slice::<UserPluginManagementRecord>(&bytes).ok()
                    })
                    .is_some_and(|current| {
                        current == legacy
                            && std::fs::read_link(installed)
                                .ok()
                                .is_some_and(|link| link == Path::new(source))
                    });
                if still_current {
                    if let Some(journal) =
                        load_user_plugin_migration_journal(&migration_state, &legacy.id)?
                    {
                        validate_user_plugin_migration_journal(&legacy, &journal)?;
                        finish_user_plugin_migration_journal(&journal)?;
                        remove_user_plugin_migration_journal(&migration_state, &legacy.id)?;
                    }
                }
            }
            continue;
        }
        if validate_record(&workspace, &legacy).is_err() {
            continue;
        }
        let cache_root = Path::new(&workspace.root_path)
            .join("plugins/cache")
            .join(&legacy.marketplace_name)
            .join(&legacy.install_id);
        let operation_id = format!("user-plugin-source-migration:{}", uuid::Uuid::new_v4());
        let _lock = super::TargetLockSet::acquire_for_ad_states(
            &[
                cache_root.clone(),
                migration_state
                    .user_plugin_management()
                    .display_path()
                    .join(record_file_name(&legacy.id)),
                super::resource_lifecycle_lock_target(&migration_state, &legacy.resource_id),
            ],
            &operation_id,
            &migration_state,
        )
        .map_err(|error| plugin_error(&workspace, AgentErrorCode::Io, error.to_string()))?;
        let current_record = migration_state
            .user_plugin_management()
            .read(&record_file_name(&legacy.id))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<UserPluginManagementRecord>(&bytes).ok());
        if current_record.as_ref() != Some(&legacy) || validate_record(&workspace, &legacy).is_err()
        {
            continue;
        }
        let resolved = match resolve_catalog_resource(&legacy.resource_id) {
            Ok(resolved) => resolved,
            Err(_) => continue,
        };
        let existing_journal = load_user_plugin_migration_journal(&migration_state, &legacy.id)?;
        if let Some(journal) = existing_journal.as_ref() {
            validate_user_plugin_migration_journal(&legacy, journal)?;
            recover_user_plugin_migration_journal(journal);
        }
        let mut versions = if let Some(journal) = existing_journal.as_ref() {
            vec![PathBuf::from(&journal.target_path)]
        } else {
            match std::fs::read_dir(&cache_root) {
                Ok(entries) => entries
                    .flatten()
                    .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
                    .map(|entry| entry.path())
                    .filter(|path| {
                        std::fs::symlink_metadata(path)
                            .is_ok_and(|meta| meta.is_dir() || meta.file_type().is_symlink())
                    })
                    .collect::<Vec<_>>(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(plugin_error(
                        &workspace,
                        AgentErrorCode::Io,
                        error.to_string(),
                    ))
                }
            }
        };
        versions.sort();
        if versions.len() != 1 {
            continue;
        }
        let target = versions.remove(0);
        if !linked_record_cache_path_valid(&workspace, &legacy, &target) {
            continue;
        }
        let source_digest = super::directory_tree_digest(&resolved.physical_path)
            .map_err(|error| plugin_error(&workspace, AgentErrorCode::Io, error.to_string()))?;
        let source_binding = load_resource_catalog_snapshot()
            .ok()
            .and_then(|catalog| catalog.sources.get(&resolved.source_id).cloned())
            .and_then(|source| source.binding);
        let already_linked = std::fs::read_link(&target)
            .ok()
            .is_some_and(|link| link == resolved.stable_path);
        if !already_linked
            && inspect_tree(&target, ArtifactLimits::default())
                .and_then(|manifest| manifest.digest())
                .ok()
                != Some(legacy.artifact_digest.clone())
        {
            continue;
        }
        let journal = match existing_journal {
            Some(journal) => journal,
            None => begin_user_plugin_migration_journal(
                &migration_state,
                &legacy,
                &target,
                &resolved.stable_path,
                &source_digest,
            )?,
        };
        let backup = PathBuf::from(&journal.backup_path);
        if !already_linked {
            std::fs::rename(&target, &backup)
                .map_err(|error| plugin_error(&workspace, AgentErrorCode::Io, error.to_string()))?;
            if let Err(error) = symlink(&resolved.stable_path, &target) {
                let _ = std::fs::rename(&backup, &target);
                return Err(plugin_error(
                    &workspace,
                    AgentErrorCode::Io,
                    error.to_string(),
                ));
            }
        }
        let mut record = legacy;
        record.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
        record.artifact_path = resolved.stable_path.to_string_lossy().into_owned();
        record.artifact_digest = source_digest.clone();
        record.tracking_policy = ResourceTrackingPolicy::Latest;
        record.resolved_source_revision = source_binding
            .as_ref()
            .map(|binding| binding.source_revision.clone());
        record.resolved_content_digest = Some(source_digest);
        record.linked_source_path = Some(resolved.stable_path.to_string_lossy().into_owned());
        record.linked_install_path = Some(target.to_string_lossy().into_owned());
        record.projection_entries = None;
        record.updated_at = Utc::now();
        if let Err(error) = write_record(&record) {
            if !already_linked {
                let _ = std::fs::remove_file(&target);
                let _ = std::fs::rename(&backup, &target);
            }
            return Err(plugin_error(
                &workspace,
                AgentErrorCode::Io,
                error.to_string(),
            ));
        }
        finish_user_plugin_migration_journal(&journal)?;
        remove_user_plugin_migration_journal(&migration_state, &record.id)?;
        migrated += 1;
    }
    Ok(migrated)
}

fn user_plugin_migration_journal_name(id: &OwnershipRecordId) -> String {
    format!("user-plugin-{}.json", id.as_str().replace(':', "_"))
}

fn load_user_plugin_migration_journal(
    state: &ExecutionState,
    id: &OwnershipRecordId,
) -> Result<Option<UserPluginMigrationJournal>, AgentError> {
    let bytes = match state
        .source_tracking_migration_journals()
        .read(&user_plugin_migration_journal_name(id))
    {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(global_plugin_error(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(global_plugin_error)
}

fn begin_user_plugin_migration_journal(
    state: &ExecutionState,
    record: &UserPluginManagementRecord,
    target: &Path,
    source: &Path,
    next_digest: &ContentDigest,
) -> Result<UserPluginMigrationJournal, AgentError> {
    let backup = target.with_file_name(format!(
        ".{}.ad-source-migration-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin"),
        uuid::Uuid::new_v4()
    ));
    let journal = UserPluginMigrationJournal {
        schema_version: USER_PLUGIN_MIGRATION_JOURNAL_VERSION,
        record_id: record.id.clone(),
        target_path: target.to_string_lossy().into_owned(),
        source_path: source.to_string_lossy().into_owned(),
        backup_path: backup.to_string_lossy().into_owned(),
        previous_digest: record.artifact_digest.clone(),
        next_digest: next_digest.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&journal).map_err(global_plugin_error)?;
    state
        .source_tracking_migration_journals()
        .write_atomic_new(&user_plugin_migration_journal_name(&record.id), &bytes)
        .map_err(global_plugin_error)?;
    Ok(journal)
}

#[cfg(test)]
pub(super) fn persist_user_plugin_migration_journal_for_test(
    record: &UserPluginManagementRecord,
    target: &Path,
    source: &Path,
) -> Result<std::path::PathBuf, AgentError> {
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let next_digest = super::directory_tree_digest(source).map_err(global_plugin_error)?;
    begin_user_plugin_migration_journal(&state, record, target, source, &next_digest)
        .map(|journal| std::path::PathBuf::from(journal.backup_path))
}

fn validate_user_plugin_migration_journal(
    record: &UserPluginManagementRecord,
    journal: &UserPluginMigrationJournal,
) -> Result<(), AgentError> {
    let target = Path::new(&journal.target_path);
    let backup = Path::new(&journal.backup_path);
    let valid = journal.schema_version == USER_PLUGIN_MIGRATION_JOURNAL_VERSION
        && journal.record_id == record.id
        && Path::new(&journal.source_path).is_absolute()
        && backup.parent() == target.parent()
        && backup.file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with(&format!(
                ".{}.ad-source-migration-",
                target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("plugin")
            ))
        })
        && (journal.previous_digest == record.artifact_digest
            || journal.next_digest == record.artifact_digest)
        && record
            .linked_install_path
            .as_deref()
            .map_or(true, |path| path == journal.target_path)
        && record
            .linked_source_path
            .as_deref()
            .map_or(true, |path| path == journal.source_path);
    if valid {
        Ok(())
    } else {
        Err(global_plugin_error(
            "User Plugin source-tracking migration journal is invalid",
        ))
    }
}

fn recover_user_plugin_migration_journal(journal: &UserPluginMigrationJournal) {
    let target = Path::new(&journal.target_path);
    let backup = Path::new(&journal.backup_path);
    if std::fs::symlink_metadata(target).is_err()
        && inspect_tree(backup, ArtifactLimits::default())
            .and_then(|manifest| manifest.digest())
            .ok()
            == Some(journal.previous_digest.clone())
    {
        let _ = std::fs::rename(backup, target);
    }
}

fn recover_previous_codex_migration(
    workspace: &UserWorkspaceDescriptor,
    state: &ExecutionState,
    record: &UserPluginManagementRecord,
) -> Result<(), AgentError> {
    let Some(journal) = load_user_plugin_migration_journal(state, &record.id)? else {
        return Ok(());
    };
    validate_user_plugin_migration_journal(record, &journal)?;
    if record.workspace_key != workspace.key
        || record.installation_id != workspace.installation_id
        || record.agent_id != workspace.agent_id
        || record.id
            != OwnershipRecordId::from(opaque_contract_id(
                "user-plugin-management",
                &[workspace.key.as_str(), &record.resource_id],
            ))
    {
        return Err(global_plugin_error(
            "Previous Plugin migration no longer matches its workspace",
        ));
    }
    let target_path = Path::new(&journal.target_path);
    let backup_path = Path::new(&journal.backup_path);
    let target = record_cache_target(workspace, record, target_path)?;
    let backup = record_cache_target(workspace, record, backup_path)?;
    let legacy_digest = |path: &Path| {
        inspect_tree(path, ArtifactLimits::default())
            .and_then(|tree| tree.digest())
            .map_err(global_plugin_error)
    };
    let target_state = target.observe()?;
    let restored = matches!(target_state, TargetState::Directory(_))
        && legacy_digest(target_path)? == journal.previous_digest;
    if !restored {
        let expected_link = matches!(target_state, TargetState::Symlink(ref link) if link == Path::new(&journal.source_path));
        if !matches!(target_state, TargetState::Missing) && !expected_link {
            return Err(global_plugin_error(
                "Previous Plugin migration target is occupied; recovery evidence retained",
            ));
        }
        if !matches!(backup.observe()?, TargetState::Directory(_))
            || legacy_digest(backup_path)? != journal.previous_digest
        {
            return Err(global_plugin_error(
                "Previous Plugin migration backup is unavailable or changed; journal retained",
            ));
        }
        if expected_link {
            target.remove()?;
        }
        backup.rename_to_sibling(target_path.file_name().unwrap().to_str().unwrap())?;
    }
    // The restored physical cache is the previous installation, not new Source content.
    remove_user_plugin_migration_journal(state, &record.id)?;
    if matches!(backup.observe()?, TargetState::Directory(_))
        && legacy_digest(backup_path)? == journal.previous_digest
    {
        if let Err(error) = backup.remove() {
            tracing::warn!(%error, "previous migration backup retained");
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn simulate_previous_codex_migration_for_test(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    published_link: bool,
) -> PathBuf {
    let state = ExecutionState::open().unwrap();
    let target_path = Path::new(record.linked_install_path.as_deref().unwrap());
    let backup_path = target_path.with_file_name(format!(
        ".{}.ad-source-migration-test",
        target_path.file_name().unwrap().to_str().unwrap()
    ));
    let journal = UserPluginMigrationJournal {
        schema_version: USER_PLUGIN_MIGRATION_JOURNAL_VERSION,
        record_id: record.id.clone(),
        target_path: target_path.to_string_lossy().into_owned(),
        source_path: record.linked_source_path.clone().unwrap(),
        backup_path: backup_path.to_string_lossy().into_owned(),
        previous_digest: inspect_tree(target_path, ArtifactLimits::default())
            .unwrap()
            .digest()
            .unwrap(),
        next_digest: record.artifact_digest.clone(),
    };
    state
        .source_tracking_migration_journals()
        .write_atomic_new(
            &user_plugin_migration_journal_name(&record.id),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
    let target = record_cache_target(workspace, record, target_path).unwrap();
    target
        .rename_to_sibling(backup_path.file_name().unwrap().to_str().unwrap())
        .unwrap();
    if published_link {
        target
            .write_symlink_atomic(Path::new(record.linked_source_path.as_deref().unwrap()))
            .unwrap();
    }
    backup_path
}

fn finish_user_plugin_migration_journal(
    journal: &UserPluginMigrationJournal,
) -> Result<(), AgentError> {
    let backup = Path::new(&journal.backup_path);
    if std::fs::symlink_metadata(backup).is_err() {
        return Ok(());
    }
    let digest = inspect_tree(backup, ArtifactLimits::default())
        .and_then(|manifest| manifest.digest())
        .map_err(global_plugin_error)?;
    if digest != journal.previous_digest {
        return Err(global_plugin_error(
            "User Plugin source-tracking migration backup changed",
        ));
    }
    std::fs::remove_dir_all(backup).map_err(global_plugin_error)
}

fn remove_user_plugin_migration_journal(
    state: &ExecutionState,
    id: &OwnershipRecordId,
) -> Result<(), AgentError> {
    state
        .source_tracking_migration_journals()
        .remove(&user_plugin_migration_journal_name(id))
        .map_err(global_plugin_error)
}

pub(super) fn refresh_latest_user_plugin_resolutions(source_id: &str) -> Result<(), AgentError> {
    let catalog = load_resource_catalog_snapshot().map_err(global_plugin_error)?;
    let source_revision = catalog
        .sources
        .get(source_id)
        .and_then(|source| source.binding.as_ref())
        .map(|binding| binding.source_revision.clone());
    let mut synchronized_installations = Vec::new();
    let mut issues = Vec::new();
    for mut record in list_all_user_plugin_management_records()?
        .into_iter()
        .filter(|record| {
            record.source_id == source_id
                && record.tracking_policy == ResourceTrackingPolicy::Latest
                && record.removal_phase.is_none()
        })
    {
        let Some(candidate) = catalog
            .resources
            .get(&record.resource_id)
            .filter(|candidate| {
                candidate.present && candidate.lifecycle == ResourceLifecycle::Managed
            })
        else {
            continue;
        };
        let workspace = match super::resolve_user_agent_workspace(&record.installation_id) {
            Ok(workspace) => workspace,
            Err(error) => {
                issues.push(error.to_string());
                continue;
            }
        };
        if workspace.agent_id.as_str() == "codex" {
            match repair_user_plugin_binding(&workspace, &record, false) {
                Ok(()) => {
                    if !synchronized_installations.contains(&record.installation_id) {
                        synchronized_installations.push(record.installation_id.clone());
                    }
                }
                Err(error) => issues.push(error.to_string()),
            }
            continue;
        }
        if !user_plugin_binding_current(&workspace, &record) {
            continue;
        }
        let resolved = match resolve_catalog_resource(&candidate.id) {
            Ok(resolved) => resolved,
            Err(error) => {
                issues.push(error.to_string());
                continue;
            }
        };
        let digest = match super::directory_tree_digest(&resolved.physical_path) {
            Ok(digest) => digest,
            Err(error) => {
                issues.push(error.to_string());
                continue;
            }
        };
        record.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
        record.artifact_path = resolved.stable_path.to_string_lossy().into_owned();
        record.artifact_digest = digest.clone();
        record.resolved_source_revision = source_revision.clone();
        record.resolved_content_digest = Some(digest);
        record.linked_source_path = Some(resolved.stable_path.to_string_lossy().into_owned());
        record.updated_at = Utc::now();
        if let Err(error) = write_record(&record) {
            issues.push(error.to_string());
        }
    }
    // Complete the global phase before a project reads any managed global Plugin.
    for installation in synchronized_installations {
        if let Err(error) = synchronize_inheriting_projects(&installation) {
            issues.push(error.to_string());
        }
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(global_plugin_error(issues.join("\n")))
    }
}

pub(super) fn user_plugin_binding_current(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
) -> bool {
    let (Some(source), Some(installed)) = (&record.linked_source_path, &record.linked_install_path)
    else {
        return false;
    };
    if workspace.agent_id.as_str() == "codex" {
        user_plugin_binding_owned(workspace, record)
            && super::execution_tree::tracked_source_digest(Path::new(source))
                .is_ok_and(|digest| Some(&digest) == record.cache_digest.as_ref())
    } else {
        std::fs::read_link(installed).is_ok_and(|link| link == Path::new(source))
    }
}

pub(super) fn user_plugin_binding_owned(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
) -> bool {
    if workspace.agent_id.as_str() == "codex" {
        let Some(installed) = &record.linked_install_path else {
            return false;
        };
        let Some(expected) = &record.cache_digest else {
            return false;
        };
        let Ok(target) = record_cache_target(workspace, record, Path::new(installed)) else {
            return false;
        };
        matches!(target.observe(), Ok(TargetState::Directory(ref digest)) if digest == expected)
            && super::codex_plugins::active_plugin_version(Path::new(installed).parent().unwrap())
                .as_deref()
                == Path::new(installed)
                    .file_name()
                    .and_then(|name| name.to_str())
    } else {
        user_plugin_binding_current(workspace, record)
    }
}

fn synchronize_inheriting_projects(installation_id: &InstallationId) -> Result<(), AgentError> {
    for installation in super::builtin_registry()
        .discover()
        .into_iter()
        .filter(|installation| {
            installation.agent_id.as_str() == "codex"
                && installation.base_installation_id.as_ref() == Some(installation_id)
                && installation.project_path.is_some()
        })
    {
        super::codex_plugins::synchronize_inherited_plugin_packages(&AgentContext {
            installation_id: installation.id,
            project_path: installation.project_path,
        })?;
    }
    Ok(())
}

pub fn reconcile_user_plugin_source_tracking(
    installation_id: &InstallationId,
) -> Result<(), AgentError> {
    let workspace = super::resolve_user_agent_workspace(installation_id)?;
    if workspace.agent_id.as_str() != "codex" {
        return Ok(());
    }
    for record in list_user_plugin_management_records(&workspace)? {
        if record.linked_install_path.is_some()
            && record.removal_phase.is_none()
            && resolve_catalog_resource(&record.resource_id).is_ok()
        {
            repair_user_plugin_binding(&workspace, &record, false)?;
        }
    }
    Ok(())
}

fn record_cache_target(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    installed: &Path,
) -> Result<ConfinedTarget, AgentError> {
    if !linked_record_cache_path_valid(workspace, record, installed) {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::PermissionDenied,
            "Plugin cache identity differs from the installation record",
        ));
    }
    let resource = ResourceRef {
        installation_id: record.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Plugins,
        scope: ResourceScope::User,
        logical_id: record.native_id.clone(),
    };
    ConfinedTarget::resolve_bound(&resource, installed, None)
}

/// Restore only proven bindings or a fresh version target; preserve replaced caches.
fn repair_user_plugin_binding(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    lifecycle_locked: bool,
) -> Result<(), AgentError> {
    if workspace.agent_id.as_str() != "codex" {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::Unsupported,
            "Only Codex cache repair is supported",
        ));
    }
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let cache_root = Path::new(&workspace.root_path)
        .join("plugins/cache")
        .join(&record.marketplace_name)
        .join(&record.install_id);
    let mut locks = vec![
        cache_root.clone(),
        state
            .user_plugin_management()
            .display_path()
            .join(record_file_name(&record.id)),
    ];
    if !lifecycle_locked {
        locks.push(super::resource_lifecycle_lock_target(
            &state,
            &record.resource_id,
        ));
    }
    let _lock = super::TargetLockSet::acquire_for_ad_states(
        &locks,
        &format!("plugin-binding:{}", uuid::Uuid::new_v4()),
        &state,
    )
    .map_err(global_plugin_error)?;
    let recovered = recover_plugin_cache_repair(workspace, &state, record)?;
    let record = recovered.as_ref().unwrap_or(record);
    validate_record(workspace, record)?;
    let current: UserPluginManagementRecord = serde_json::from_slice(
        &state
            .user_plugin_management()
            .read(&record_file_name(&record.id))
            .map_err(global_plugin_error)?,
    )
    .map_err(global_plugin_error)?;
    if current != *record {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            "Plugin installation changed during repair",
        ));
    }
    let resolved = resolve_user_plugin_candidate(workspace, &record.resource_id)?;
    if resolved
        .physical_path
        .starts_with(Path::new(&workspace.root_path))
    {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::PermissionDenied,
            "Linked Plugin source cannot be inside the Agent installation root",
        ));
    }
    let bytes = std::fs::read(resolved.physical_path.join(".codex-plugin/plugin.json"))
        .map_err(global_plugin_error)?;
    let descriptor: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(global_plugin_error)?;
    let version = descriptor
        .get("version")
        .and_then(serde_json::Value::as_str)
        .or(Some("local"))
        .filter(|value| {
            valid_segment(value) && (*value == "local" || semver::Version::parse(value).is_ok())
        })
        .ok_or_else(|| {
            plugin_error(
                workspace,
                AgentErrorCode::ResourceChanged,
                "Plugin source has no valid version",
            )
        })?;
    let recorded_cache_owned = user_plugin_binding_owned(workspace, record);
    let entries = match std::fs::read_dir(&cache_root) {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(global_plugin_error(error)),
    };
    if let Some(entries) = entries {
        for entry in entries {
            let entry = entry.map_err(global_plugin_error)?;
            if !entry.file_type().map_err(global_plugin_error)?.is_dir() {
                continue;
            }
            let Some(existing_version) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if version != "local"
                && !existing_version.starts_with('.')
                && existing_version != "local"
                && semver::Version::parse(&existing_version).is_err()
            {
                return Err(plugin_error(
                    workspace,
                    AgentErrorCode::PermissionDenied,
                    "An unowned Plugin cache occupies the active installation",
                ));
            }
            if !super::codex_plugins::cache_version_is_newer(&existing_version, version) {
                continue;
            }
            let recorded_path = record.linked_install_path.as_deref().map(Path::new);
            if !recorded_cache_owned || recorded_path != Some(entry.path().as_path()) {
                return Err(plugin_error(
                    workspace,
                    AgentErrorCode::PermissionDenied,
                    "A newer unowned Plugin cache occupies the active installation",
                ));
            }
        }
    }
    let desired = cache_root.join(version);
    let target = record_cache_target(workspace, record, &desired)?;
    let original = target.observe()?;
    let same_target = record
        .linked_install_path
        .as_deref()
        .is_some_and(|path| Path::new(path) == desired);
    let owned = same_target
        && match (
            &original,
            &record.projection_entries,
            &record.linked_source_path,
        ) {
            (TargetState::Directory(digest), _, _)
                if record.cache_digest.as_ref() == Some(digest) =>
            {
                true
            }
            (TargetState::Directory(_), Some(entries), Some(source)) => {
                target.source_links_match(Path::new(source), entries)?
            }
            (TargetState::Symlink(link), None, Some(source)) => link == Path::new(source),
            _ => false,
        };
    if !matches!(original, TargetState::Missing) && !owned {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::PermissionDenied,
            "Plugin repair target is occupied; the existing cache was preserved",
        ));
    }
    let mut next = record.clone();
    next.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
    next.artifact_digest =
        super::directory_tree_digest(&resolved.physical_path).map_err(global_plugin_error)?;
    next.resolved_content_digest = Some(next.artifact_digest.clone());
    next.resolved_source_revision = load_resource_catalog_snapshot()
        .map_err(global_plugin_error)?
        .sources
        .get(&record.source_id)
        .and_then(|source| source.binding.as_ref())
        .map(|binding| binding.source_revision.clone());
    next.artifact_path = resolved.stable_path.to_string_lossy().into_owned();
    next.linked_source_path = Some(next.artifact_path.clone());
    next.linked_install_path = Some(desired.to_string_lossy().into_owned());
    let digest = super::execution_tree::tracked_source_digest(&resolved.stable_path)
        .map_err(global_plugin_error)?;
    if std::fs::read(resolved.stable_path.join(".codex-plugin/plugin.json"))
        .map_err(global_plugin_error)?
        != bytes
    {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            "Plugin descriptor changed while preparing synchronization",
        ));
    }
    let already_current =
        matches!(&original, TargetState::Directory(existing) if existing == &digest) && owned;
    let backup_name = format!("cache-repair-{}", uuid::Uuid::new_v4());
    let backed_up = !already_current && matches!(original, TargetState::Directory(_));
    next.projection_entries = None;
    next.cache_digest = Some(digest.clone());
    next.updated_at = Utc::now();
    if already_current
        && record.cache_digest == next.cache_digest
        && record.resolved_source_revision == next.resolved_source_revision
        && record.artifact_digest == next.artifact_digest
        && record.schema_version == next.schema_version
    {
        return Ok(());
    }
    if backed_up {
        target.copy_directory_to(state.user_plugin_artifacts(), &backup_name)?;
    }
    let journal = UserPluginCacheRepairJournal {
        before: record.clone(),
        after: next.clone(),
        backup_name: backed_up.then(|| backup_name.clone()),
        original_digest: original.digest(),
        original_link: match &original {
            TargetState::Symlink(link) => Some(link.to_string_lossy().into_owned()),
            _ => None,
        },
    };
    let journal_name = cache_repair_journal_name(record);
    state
        .source_tracking_migration_journals()
        .write_atomic_new(
            &journal_name,
            &serde_json::to_vec(&journal).map_err(global_plugin_error)?,
        )
        .map_err(global_plugin_error)?;
    if !already_current {
        if let Err(error) = target.write_tracked_source_atomic(&resolved.stable_path, &digest) {
            recover_plugin_cache_repair(workspace, &state, record)?;
            return Err(error);
        }
    }
    if let Err(error) = write_record(&next) {
        if !already_current
            && matches!(target.observe()?, TargetState::Directory(ref current) if current == &digest)
        {
            match &original {
                TargetState::Missing => target.remove()?,
                TargetState::Symlink(link) => {
                    target.remove()?;
                    target.write_symlink_atomic(link)?;
                }
                TargetState::Directory(_) => target.write_directory_from(
                    &state
                        .user_plugin_artifacts()
                        .open_directory(&backup_name)
                        .map_err(global_plugin_error)?,
                )?,
                _ => unreachable!("only owned bindings can be replaced"),
            }
        }
        recover_plugin_cache_repair(workspace, &state, record)?;
        return Err(plugin_error(
            workspace,
            AgentErrorCode::Io,
            format!("Could not record Plugin repair: {error}"),
        ));
    }
    recover_plugin_cache_repair(workspace, &state, record)?;
    Ok(())
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
            Some(record) if record.linked_source_path.is_some() => record,
            Some(_) | None => proposed_user_plugin_record(workspace, resource_id)?,
        },
        ResourceAction::Update
        | ResourceAction::Enable
        | ResourceAction::Disable
        | ResourceAction::Remove => existing.ok_or_else(|| {
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
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let mut lock_targets = vec![super::resource_lifecycle_lock_target(
        &state,
        &plan.record.resource_id,
    )];
    if plan.record.agent_id.as_str() == "codex" {
        lock_targets.push(Path::new(&plan.root_path).join("config.toml"));
    }
    let _lifecycle_lock =
        super::TargetLockSet::acquire_for_ad_states(&lock_targets, plan_id.as_str(), &state)
            .map_err(|error| {
                plan_error(
                    &plan,
                    if error.kind() == std::io::ErrorKind::WouldBlock {
                        AgentErrorCode::ResourceChanged
                    } else {
                        AgentErrorCode::Io
                    },
                    error.to_string(),
                )
            })?;
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
    drop(_lifecycle_lock);
    let mut issues = Vec::new();
    if plan.action == NativePluginAction::Repair {
        if let Err(error) = synchronize_inheriting_projects(&plan.record.installation_id) {
            tracing::warn!(%error, "global Plugin updated; inherited project synchronization failed");
            issues.push(super::WorkspaceOperationIssue {
                code: "inherited_plugin_sync_failed".into(),
                message_key: "agents.resources.actionPartialFailure".into(),
                resource_key: None,
            });
        }
    }
    Ok(WorkspaceOperationReport {
        workspace_key: plan.workspace_key,
        outcome: if issues.is_empty() {
            WorkspaceOperationOutcome::Changed
        } else {
            WorkspaceOperationOutcome::PartialFailure
        },
        issues,
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
            if plan.record_preexisting {
                return Err(plan_error(
                    plan,
                    AgentErrorCode::PermissionDenied,
                    "Uninstall the existing Plugin binding before installing again",
                ));
            }
            if plan.record.agent_id.as_str() == "codex" {
                let cache_root = Path::new(&plan.root_path)
                    .join("plugins/cache")
                    .join(&plan.record.marketplace_name)
                    .join(&plan.record.install_id);
                let resource = ResourceRef {
                    installation_id: plan.record.installation_id.clone(),
                    project_path: None,
                    kind: ResourceKind::Plugins,
                    scope: ResourceScope::User,
                    logical_id: plan.record.native_id.clone(),
                };
                let target = ConfinedTarget::resolve_bound(&resource, &cache_root, None)?;
                if std::fs::symlink_metadata(&cache_root).is_ok()
                    && !target.source_links_match(Path::new(&plan.record.artifact_path), &[])?
                {
                    return Err(plan_error(
                        plan,
                        AgentErrorCode::PermissionDenied,
                        "Plugin cache is occupied; existing content was preserved",
                    ));
                }
            }
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
            let output = match run_required(plan, runner, &install).await {
                Ok(output) => output,
                Err(error) => {
                    if !plan.record_preexisting {
                        compensate_new_plugin_install(plan, runner, true).await;
                    }
                    return Err(error);
                }
            };
            let binding = match link_installed_plugin(plan, &output) {
                Ok(binding) => binding,
                Err(error) => {
                    compensate_new_plugin_install(plan, runner, true).await;
                    return Err(error);
                }
            };
            let record = installed_plugin_record(plan, binding);
            if let Err(error) = write_record(&record) {
                if let Some(path) = record.linked_install_path.as_deref() {
                    let _ = restore_linked_plugin_cache(plan, Path::new(path), false);
                }
                compensate_new_plugin_install(plan, runner, true).await;
                return Err(plan_error(
                    plan,
                    AgentErrorCode::Io,
                    format!("Failed to record source-tracking Plugin installation: {error}"),
                ));
            }
        }
        NativePluginAction::Enable | NativePluginAction::Disable => {
            let args = plugin_action_args(&plan.record, plan.action);
            run_required(plan, runner, &args).await?;
        }
        NativePluginAction::Repair => {
            let workspace = super::resolve_user_agent_workspace(&plan.record.installation_id)?;
            repair_user_plugin_binding(&workspace, &plan.record, true)?;
        }
        NativePluginAction::Remove => {
            let mut removal_plan = plan.clone();
            let resuming_removal = removal_plan.record.removal_phase.is_some();
            preserve_owned_codex_cache(&removal_plan, resuming_removal)?;
            if removal_plan.record.removal_phase.is_none() {
                removal_plan.record = persist_removal_phase(
                    &removal_plan,
                    UserPluginRemovalPhase::LinkRemovalPending,
                )?;
            }
            let removed_link = match remove_linked_plugin_cache(&removal_plan, resuming_removal) {
                Ok(removed) => removed,
                Err(error) => {
                    if !resuming_removal {
                        clear_removal_phase(&removal_plan)?;
                    }
                    return Err(error);
                }
            };
            if removal_plan.record.removal_phase < Some(UserPluginRemovalPhase::LinkRemoved) {
                removal_plan.record =
                    persist_removal_phase(&removal_plan, UserPluginRemovalPhase::LinkRemoved)?;
            }
            let args = plugin_action_args(&removal_plan.record, NativePluginAction::Remove);
            if removal_plan.record.removal_phase < Some(UserPluginRemovalPhase::NativeRemoved) {
                let unregister = if removal_plan.record.agent_id.as_str() == "codex" {
                    preserve_remaining_codex_cache(&removal_plan)
                        .and_then(|_| unregister_codex_plugin(&removal_plan))
                } else {
                    run_required_or_missing(&removal_plan, runner, &args)
                        .await
                        .map(|_| ())
                };
                if let Err(error) = unregister {
                    if removal_plan.record.agent_id.as_str() == "codex" {
                        restore_remaining_codex_cache(&removal_plan)?;
                    }
                    if removed_link || resuming_removal {
                        if let Some(path) = removal_plan.record.linked_install_path.as_deref() {
                            let restored_cache = if removal_plan.record.cache_digest.is_some() {
                                restore_owned_codex_cache(&removal_plan, Path::new(path))
                            } else {
                                restore_linked_plugin_cache(&removal_plan, Path::new(path), true)
                            };
                            match restored_cache {
                                Ok(()) => {
                                    let mut restored = removal_plan.record.clone();
                                    restored.removal_phase = None;
                                    restored.updated_at = Utc::now();
                                    if let Err(restore_error) = write_record(&restored) {
                                        return Err(plan_error(
                                        &removal_plan,
                                        AgentErrorCode::Io,
                                        format!(
                                            "Plugin uninstall failed and restored state could not be recorded: {restore_error}"
                                        ),
                                    ));
                                    }
                                    cleanup_owned_codex_cache_backup(&removal_plan);
                                }
                                Err(restore_error) => {
                                    return Err(plan_error(
                                    &removal_plan,
                                    AgentErrorCode::Io,
                                    format!(
                                        "Plugin uninstall failed and its cache link could not be restored: {restore_error}"
                                    ),
                                ));
                                }
                            }
                        }
                    }
                    return Err(error);
                }
            }
            if removal_plan.record.removal_phase < Some(UserPluginRemovalPhase::NativeRemoved) {
                removal_plan.record =
                    persist_removal_phase(&removal_plan, UserPluginRemovalPhase::NativeRemoved)?;
            }
            if removal_plan.record.removal_phase < Some(UserPluginRemovalPhase::MarketplaceRemoved)
            {
                let remove_marketplace = marketplace_remove_args(&removal_plan.record);
                if removal_plan.record.agent_id.as_str() != "codex" {
                    run_required_or_missing(&removal_plan, runner, &remove_marketplace).await?;
                }
                removal_plan.record = persist_removal_phase(
                    &removal_plan,
                    UserPluginRemovalPhase::MarketplaceRemoved,
                )?;
            }
            remove_record(&removal_plan.record.id).map_err(|error| {
                plan_error(
                    &removal_plan,
                    AgentErrorCode::Io,
                    format!("Failed to clear Plugin ownership: {error}"),
                )
            })?;
            cleanup_owned_codex_cache_backup(&removal_plan);
        }
    }
    Ok(())
}

fn codex_removal_backup_name(plan: &StoredUserPluginPlan) -> String {
    let identity = format!(
        "{}:{}:{}",
        plan.record.id,
        plan.record.created_at,
        plan.record
            .cache_digest
            .as_ref()
            .map(ContentDigest::as_str)
            .unwrap_or("")
    );
    format!(
        "cache-removal-{}",
        ContentDigest::sha256(identity.as_bytes())
            .as_str()
            .trim_start_matches("sha256:")
    )
}

fn preserve_owned_codex_cache(
    plan: &StoredUserPluginPlan,
    allow_missing: bool,
) -> Result<(), AgentError> {
    if plan.record.agent_id.as_str() != "codex" {
        return Ok(());
    }
    let Some(expected) = &plan.record.cache_digest else {
        return Ok(());
    };
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let name = codex_removal_backup_name(plan);
    match state.user_plugin_artifacts().open_directory(&name) {
        Ok(backup) => {
            if backup.digest().map_err(global_plugin_error)? != *expected {
                return Err(global_plugin_error(
                    "Plugin removal backup changed; backup retained",
                ));
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(global_plugin_error(error)),
    }
    let path = plan
        .record
        .linked_install_path
        .as_deref()
        .ok_or_else(|| global_plugin_error("Missing owned Plugin cache path"))?;
    let target = linked_cache_target(plan, Path::new(path))?;
    match target.observe()? {
        TargetState::Directory(ref digest) if digest == expected => {
            target.copy_directory_to(state.user_plugin_artifacts(), &name)?;
            let backup = state
                .user_plugin_artifacts()
                .open_directory(&name)
                .map_err(global_plugin_error)?;
            if backup.digest().map_err(global_plugin_error)? != *expected {
                return Err(global_plugin_error(
                    "Plugin changed while preparing removal backup",
                ));
            }
            Ok(())
        }
        TargetState::Missing
            if allow_missing
                && plan.record.removal_phase >= Some(UserPluginRemovalPhase::NativeRemoved) =>
        {
            Ok(())
        }
        _ => Err(global_plugin_error(
            "Owned Plugin cache or removal backup is unavailable",
        )),
    }
}

fn restore_owned_codex_cache(plan: &StoredUserPluginPlan, path: &Path) -> Result<(), AgentError> {
    let state = ExecutionState::open().map_err(global_plugin_error)?;
    let backup = state
        .user_plugin_artifacts()
        .open_directory(&codex_removal_backup_name(plan))
        .map_err(global_plugin_error)?;
    if Some(backup.digest().map_err(global_plugin_error)?) != plan.record.cache_digest {
        return Err(global_plugin_error(
            "Plugin removal backup changed; backup retained",
        ));
    }
    let target = linked_cache_target(plan, path)?;
    match target.observe()? {
        TargetState::Missing => target.write_directory_from(&backup),
        TargetState::Directory(ref digest) if Some(digest) == plan.record.cache_digest.as_ref() => {
            Ok(())
        }
        _ => Err(global_plugin_error(
            "Plugin cache became occupied during uninstall; backup retained",
        )),
    }
}

fn cleanup_owned_codex_cache_backup(plan: &StoredUserPluginPlan) {
    if plan.record.agent_id.as_str() != "codex" || plan.record.cache_digest.is_none() {
        return;
    }
    let cleanup = (|| -> Result<(), AgentError> {
        let state = ExecutionState::open().map_err(global_plugin_error)?;
        let name = codex_removal_backup_name(plan);
        let backup = match state.user_plugin_artifacts().open_directory(&name) {
            Ok(backup) => backup,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(global_plugin_error(error)),
        };
        if Some(backup.digest().map_err(global_plugin_error)?) != plan.record.cache_digest {
            return Err(global_plugin_error(
                "Plugin removal backup changed; backup retained",
            ));
        }
        state
            .user_plugin_artifacts()
            .remove(&name)
            .map_err(global_plugin_error)
    })();
    if let Err(error) = cleanup {
        tracing::warn!(%error, "Plugin removal backup retained");
    }
}

fn codex_cache_retention_paths(plan: &StoredUserPluginPlan) -> (PathBuf, PathBuf) {
    let root = Path::new(&plan.root_path)
        .join("plugins/cache")
        .join(&plan.record.marketplace_name)
        .join(&plan.record.install_id);
    let digest =
        ContentDigest::sha256(format!("{}:{}", plan.record.id, plan.record.created_at).as_bytes());
    let retained = root.with_file_name(format!(
        ".ad-retained-{}",
        digest.as_str().trim_start_matches("sha256:")
    ));
    (root, retained)
}

fn preserve_remaining_codex_cache(plan: &StoredUserPluginPlan) -> Result<(), AgentError> {
    let (root, retained) = codex_cache_retention_paths(plan);
    let resource = ResourceRef {
        installation_id: plan.record.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Plugins,
        scope: ResourceScope::User,
        logical_id: plan.record.native_id.clone(),
    };
    let target = ConfinedTarget::resolve_bound(&resource, &root, None)?;
    match target.observe()? {
        TargetState::Missing => Ok(()),
        TargetState::Directory(_) => {
            target.rename_to_sibling(retained.file_name().unwrap().to_str().unwrap())?;
            tracing::info!(path = %retained.display(), "retained remaining Plugin cache during uninstall");
            Ok(())
        }
        _ => Err(plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Plugin cache parent no longer matches the installation",
        )),
    }
}

fn restore_remaining_codex_cache(plan: &StoredUserPluginPlan) -> Result<(), AgentError> {
    let (root, retained) = codex_cache_retention_paths(plan);
    let resource = ResourceRef {
        installation_id: plan.record.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Plugins,
        scope: ResourceScope::User,
        logical_id: plan.record.native_id.clone(),
    };
    let target = ConfinedTarget::resolve_bound(&resource, &retained, None)?;
    if matches!(target.observe()?, TargetState::Directory(_)) {
        target.rename_to_sibling(root.file_name().unwrap().to_str().unwrap())?;
    }
    Ok(())
}

/// Native remove erases all cache versions; prune owned registration instead.
fn unregister_codex_plugin(plan: &StoredUserPluginPlan) -> Result<(), AgentError> {
    let resource = ResourceRef {
        installation_id: plan.record.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Plugins,
        scope: ResourceScope::User,
        logical_id: plan.record.native_id.clone(),
    };
    let target = ConfinedTarget::resolve_bound(
        &resource,
        &Path::new(&plan.root_path).join("config.toml"),
        None,
    )?;
    let bytes = match target.observe()? {
        TargetState::File(bytes) => bytes,
        TargetState::Missing => return Ok(()),
        _ => {
            return Err(plan_error(
                plan,
                AgentErrorCode::PermissionDenied,
                "Plugin config target is not a file",
            ))
        }
    };
    let mut value = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.parse::<toml::Value>().ok())
        .ok_or_else(|| {
            plan_error(
                plan,
                AgentErrorCode::InvalidPlan,
                "Plugin config is invalid",
            )
        })?;
    if let Some(plugins) = value.get_mut("plugins").and_then(toml::Value::as_table_mut) {
        plugins.remove(&plan.record.native_id);
    }
    let shared = value
        .get("plugins")
        .and_then(toml::Value::as_table)
        .is_some_and(|plugins| {
            plugins.keys().any(|id| {
                id.split_once('@')
                    .is_some_and(|(_, marketplace)| marketplace == plan.record.marketplace_name)
            })
        });
    if !shared {
        if let Some(marketplaces) = value
            .get_mut("marketplaces")
            .and_then(toml::Value::as_table_mut)
        {
            let owned =
                marketplaces
                    .get(&plan.record.marketplace_name)
                    .is_some_and(|marketplace| {
                        marketplace.get("source_type").and_then(toml::Value::as_str)
                            == Some("local")
                            && marketplace.get("source").and_then(toml::Value::as_str)
                                == Some(plan.record.marketplace_root.as_str())
                    });
            if owned {
                marketplaces.remove(&plan.record.marketplace_name);
            }
        }
    }
    let content = toml::to_string_pretty(&value)
        .map_err(|error| plan_error(plan, AgentErrorCode::Io, error.to_string()))?;
    target.write_atomic(content.as_bytes())
}

fn persist_removal_phase(
    plan: &StoredUserPluginPlan,
    phase: UserPluginRemovalPhase,
) -> Result<UserPluginManagementRecord, AgentError> {
    let mut record = plan.record.clone();
    record.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
    record.removal_phase = Some(phase);
    record.updated_at = Utc::now();
    write_record(&record).map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::Io,
            format!("Failed to record Plugin removal progress: {error}"),
        )
    })?;
    Ok(record)
}

fn clear_removal_phase(
    plan: &StoredUserPluginPlan,
) -> Result<UserPluginManagementRecord, AgentError> {
    let mut record = plan.record.clone();
    record.schema_version = USER_PLUGIN_RECORD_SCHEMA_VERSION;
    record.removal_phase = None;
    record.updated_at = Utc::now();
    write_record(&record).map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::Io,
            format!("Failed to clear Plugin removal progress: {error}"),
        )
    })?;
    Ok(record)
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
    if let Some(source) = plan.record.linked_source_path.as_deref() {
        let physical = std::fs::canonicalize(source).map_err(|error| {
            plan_error(
                plan,
                AgentErrorCode::ResourceChanged,
                format!("Source-tracking Plugin is unavailable: {error}"),
            )
        })?;
        return physical.is_dir().then_some(()).ok_or_else(|| {
            plan_error(
                plan,
                AgentErrorCode::ResourceChanged,
                "Source-tracking Plugin is not a directory",
            )
        });
    }
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

struct InstalledPluginBinding {
    path: String,
    cache_digest: Option<ContentDigest>,
}

fn installed_plugin_record(
    plan: &StoredUserPluginPlan,
    binding: InstalledPluginBinding,
) -> UserPluginManagementRecord {
    let mut record = plan.record.clone();
    record.linked_install_path = Some(binding.path);
    record.cache_digest = binding.cache_digest;
    record.updated_at = Utc::now();
    record
}

#[cfg(test)]
pub(super) fn publish_install_then_edit_source_for_test(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    installed: &Path,
    edit: impl FnOnce(),
) {
    let store = UserPluginPlanStore::default();
    let view = store
        .insert(
            workspace,
            InventoryRevision::from("fixture"),
            ResourceKey::from("fixture"),
            ResourceAction::Install,
            record.clone(),
            false,
        )
        .unwrap();
    let plan = store.plans.lock().unwrap()[&view.id].clone();
    let output = CommandOutput {
        success: true,
        stdout: serde_json::json!({"installedPath": installed}).to_string(),
        stderr: String::new(),
    };
    let binding = link_installed_plugin(&plan, &output).unwrap();
    edit();
    write_record(&installed_plugin_record(&plan, binding)).unwrap();
}

fn link_installed_plugin(
    plan: &StoredUserPluginPlan,
    output: &CommandOutput,
) -> Result<InstalledPluginBinding, AgentError> {
    let installed_path = installed_plugin_path(plan, output)?;
    let source = plan.record.linked_source_path.as_deref().ok_or_else(|| {
        plan_error(
            plan,
            AgentErrorCode::InvalidPlan,
            "Linked Plugin plan has no local source",
        )
    })?;
    let source = Path::new(source);
    let physical_source = std::fs::canonicalize(source).map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::ResourceChanged,
            format!("Linked Plugin source is unavailable: {error}"),
        )
    })?;
    let root = std::fs::canonicalize(&plan.root_path).map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::Io,
            format!("Invalid Agent root: {error}"),
        )
    })?;
    if physical_source.starts_with(&root) {
        return Err(plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Linked Plugin source cannot be inside the Agent installation root",
        ));
    }
    let target = linked_cache_target(plan, &installed_path)?;
    if !matches!(target.observe()?, TargetState::Directory(_)) {
        return Err(plan_error(
            plan,
            AgentErrorCode::ResourceChanged,
            "Agent Plugin install did not create the expected cache directory",
        ));
    }
    if plan.record.agent_id.as_str() == "codex" {
        let read_descriptor =
            |root: &Path| match std::fs::read(root.join(".codex-plugin/plugin.json")) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(plan_error(
                    plan,
                    AgentErrorCode::ResourceChanged,
                    error.to_string(),
                )),
            };
        let descriptor_version = |bytes: &Option<Vec<u8>>| -> Result<Option<String>, AgentError> {
            let Some(bytes) = bytes else { return Ok(None) };
            let descriptor: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                plan_error(plan, AgentErrorCode::ResourceChanged, error.to_string())
            })?;
            Ok(descriptor
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned))
        };
        let descriptor = read_descriptor(source)?;
        let source_version = descriptor_version(&descriptor)?;
        if source_version != descriptor_version(&read_descriptor(&installed_path)?)?
            || source_version.as_deref().is_some_and(|version| {
                installed_path.file_name().and_then(|name| name.to_str()) != Some(version)
            })
        {
            return Err(plan_error(
                plan,
                AgentErrorCode::ResourceChanged,
                "Plugin source version changed after native installation",
            ));
        }
        let digest =
            super::execution_tree::tracked_source_digest(source).map_err(global_plugin_error)?;
        if read_descriptor(source)? != descriptor {
            return Err(plan_error(
                plan,
                AgentErrorCode::ResourceChanged,
                "Plugin source descriptor changed during publication",
            ));
        }
        target.write_tracked_source_atomic(source, &digest)?;
        return Ok(InstalledPluginBinding {
            path: installed_path.to_string_lossy().into_owned(),
            cache_digest: Some(digest),
        });
    }
    target.remove()?;
    if let Err(error) = target.write_symlink_atomic(source) {
        let _ =
            target.write_directory_atomic_filtered(Path::new(&plan.record.artifact_path), false);
        return Err(error);
    }
    Ok(InstalledPluginBinding {
        path: installed_path.to_string_lossy().into_owned(),
        cache_digest: None,
    })
}

fn installed_plugin_path(
    plan: &StoredUserPluginPlan,
    output: &CommandOutput,
) -> Result<std::path::PathBuf, AgentError> {
    let path = if plan.view.agent_id.as_str() == "codex" {
        serde_json::from_str::<serde_json::Value>(&output.stdout)
            .ok()
            .and_then(|value| {
                value
                    .get("installedPath")
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::PathBuf::from)
            })
    } else {
        let manifest = Path::new(&plan.root_path).join("plugins/installed_plugins.json");
        std::fs::read(&manifest)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .get("plugins")?
                    .get(&plan.record.native_id)?
                    .as_array()?
                    .iter()
                    .find(|entry| {
                        entry.get("scope").and_then(serde_json::Value::as_str) == Some("user")
                    })?
                    .get("installPath")?
                    .as_str()
                    .map(std::path::PathBuf::from)
            })
    }
    .ok_or_else(|| {
        plan_error(
            plan,
            AgentErrorCode::InvalidPlan,
            "Agent Plugin install did not report its cache path",
        )
    })?;
    let path = std::fs::canonicalize(&path).map_err(|error| {
        plan_error(
            plan,
            AgentErrorCode::ResourceChanged,
            format!("Agent Plugin cache is unavailable: {error}"),
        )
    })?;
    validate_plugin_cache_path(plan, &path)?;
    Ok(path)
}

fn validate_plugin_cache_path(
    plan: &StoredUserPluginPlan,
    installed_path: &Path,
) -> Result<(), AgentError> {
    let cache = std::fs::canonicalize(Path::new(&plan.root_path).join("plugins/cache")).map_err(
        |error| {
            plan_error(
                plan,
                AgentErrorCode::ResourceChanged,
                format!("Agent Plugin cache root is unavailable: {error}"),
            )
        },
    )?;
    let relative = installed_path.strip_prefix(&cache).map_err(|_| {
        plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Agent reported a Plugin cache path outside its installation root",
        )
    })?;
    let components = relative.components().collect::<Vec<_>>();
    let valid = components.len() == 3
        && components[0].as_os_str() == plan.record.marketplace_name.as_str()
        && components[1].as_os_str() == plan.record.install_id.as_str()
        && matches!(components[2], std::path::Component::Normal(_));
    if valid {
        Ok(())
    } else {
        Err(plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Agent reported an unexpected Plugin cache identity",
        ))
    }
}

fn linked_cache_target(
    plan: &StoredUserPluginPlan,
    installed_path: &Path,
) -> Result<ConfinedTarget, AgentError> {
    validate_plugin_cache_path(plan, installed_path)?;
    let resource = ResourceRef {
        installation_id: plan.record.installation_id.clone(),
        project_path: None,
        kind: ResourceKind::Plugins,
        scope: ResourceScope::User,
        logical_id: plan.record.native_id.clone(),
    };
    ConfinedTarget::resolve_bound(&resource, installed_path, None)
}

fn remove_linked_plugin_cache(
    plan: &StoredUserPluginPlan,
    allow_missing: bool,
) -> Result<bool, AgentError> {
    let Some(installed_path) = plan.record.linked_install_path.as_deref() else {
        return Ok(false);
    };
    let target = linked_cache_target(plan, Path::new(installed_path))?;
    if let Some(expected) = &plan.record.cache_digest {
        match target.observe()? {
            TargetState::Directory(ref digest) if digest == expected => {
                target.remove()?;
                return Ok(true);
            }
            TargetState::Missing if allow_missing => return Ok(false),
            _ => {
                return Err(plan_error(
                    plan,
                    AgentErrorCode::PermissionDenied,
                    "Linked Plugin cache no longer matches AD's installation record",
                ))
            }
        }
    }
    if let (Some(entries), Some(source)) = (
        &plan.record.projection_entries,
        &plan.record.linked_source_path,
    ) {
        if target.source_links_match(Path::new(source), entries)? {
            target.remove()?;
            return Ok(true);
        }
        if allow_missing && matches!(target.observe()?, TargetState::Missing) {
            return Ok(false);
        }
        return Err(plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Linked Plugin cache no longer matches AD's installation record",
        ));
    }
    match target.observe()? {
        TargetState::Missing if allow_missing => Ok(false),
        TargetState::Symlink(link)
            if plan.record.linked_source_path.as_deref() == link.to_str() =>
        {
            target.remove()?;
            Ok(true)
        }
        _ => Err(plan_error(
            plan,
            AgentErrorCode::PermissionDenied,
            "Linked Plugin cache no longer matches AD's installation record",
        )),
    }
}

fn restore_linked_plugin_cache(
    plan: &StoredUserPluginPlan,
    installed_path: &Path,
    as_link: bool,
) -> Result<(), AgentError> {
    if as_link && plan.record.cache_digest.is_some() {
        return restore_owned_codex_cache(plan, installed_path);
    }
    let target = linked_cache_target(plan, installed_path)?;
    target.remove()?;
    if as_link {
        let source = plan.record.linked_source_path.as_deref().ok_or_else(|| {
            plan_error(
                plan,
                AgentErrorCode::InvalidPlan,
                "Linked Plugin record has no local source",
            )
        })?;
        if plan.record.projection_entries.is_some() {
            target
                .write_source_links_atomic(Path::new(source))
                .map(|_| ())
        } else {
            target.write_symlink_atomic(Path::new(source))
        }
    } else {
        target.write_directory_atomic_filtered(Path::new(&plan.record.artifact_path), false)
    }
}

async fn run_required<R: PluginCommandRunner>(
    plan: &StoredUserPluginPlan,
    runner: &R,
    args: &[String],
) -> Result<CommandOutput, AgentError> {
    let output = runner
        .run(&plan.view.agent_id, &plan.root_path, args)
        .await
        .map_err(|error| plan_error(plan, AgentErrorCode::Io, error))?;
    if output.success {
        Ok(output)
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
    let artifact_digest =
        super::directory_tree_digest(&resolved.physical_path).map_err(|error| {
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
    let source_binding = load_resource_catalog_snapshot()
        .ok()
        .and_then(|catalog| catalog.sources.get(&resolved.source_id).cloned())
        .and_then(|source| source.binding);
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
        artifact_path: resolved.stable_path.to_string_lossy().into_owned(),
        artifact_digest: artifact_digest.clone(),
        tracking_policy: ResourceTrackingPolicy::Latest,
        resolved_source_revision: source_binding
            .as_ref()
            .map(|binding| binding.source_revision.clone()),
        resolved_content_digest: Some(artifact_digest.clone()),
        linked_source_path: Some(resolved.stable_path.to_string_lossy().into_owned()),
        linked_install_path: None,
        projection_entries: None,
        cache_digest: None,
        removal_phase: None,
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

#[cfg(test)]
pub(super) fn publish_user_plugin_artifact(
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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
    let plugin_source = record
        .linked_source_path
        .as_deref()
        .unwrap_or(&record.artifact_path);
    match std::fs::symlink_metadata(&plugin_link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(&plugin_link)? != Path::new(plugin_source) {
                std::fs::remove_file(&plugin_link)?;
                std::os::unix::fs::symlink(plugin_source, &plugin_link)?;
            }
        }
        Ok(_) => {
            return Err(std::io::Error::other(
                "Plugin marketplace target is occupied",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(plugin_source, &plugin_link)?;
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
    validate_record_with_source_requirement(workspace, record, true)
}

fn validate_record_with_source_requirement(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    require_source: bool,
) -> Result<(), AgentError> {
    let state = ExecutionState::open()
        .map_err(|error| plugin_error(workspace, AgentErrorCode::Io, error.to_string()))?;
    let resolved = require_source
        .then(|| resolve_catalog_resource(&record.resource_id).ok())
        .flatten();
    let missing_candidate = if resolved.is_none() {
        load_resource_catalog_snapshot()
            .ok()
            .and_then(|catalog| catalog.resources.get(&record.resource_id).cloned())
            .filter(|candidate| {
                candidate.kind == ResourceKind::Plugins
                    && candidate.lifecycle == ResourceLifecycle::Managed
                    && (!require_source || !candidate.present)
            })
    } else {
        None
    };
    if resolved.is_none() && missing_candidate.is_none() {
        return Err(plugin_error(
            workspace,
            AgentErrorCode::ResourceChanged,
            "User Plugin catalog resource is unavailable",
        ));
    }
    let artifact_path = Path::new(&record.artifact_path);
    let schema_valid = matches!(
        record.schema_version,
        LEGACY_USER_PLUGIN_RECORD_SCHEMA_VERSION
            | SOURCE_TRACKING_USER_PLUGIN_RECORD_SCHEMA_VERSION
            | RESOLVED_METADATA_USER_PLUGIN_RECORD_SCHEMA_VERSION
            | REMOVAL_USER_PLUGIN_RECORD_SCHEMA_VERSION
            | PROJECTION_USER_PLUGIN_RECORD_SCHEMA_VERSION
            | USER_PLUGIN_RECORD_SCHEMA_VERSION
    );
    let link_valid = match record.linked_source_path.as_deref() {
        None => record.linked_install_path.is_none(),
        Some(source) => {
            matches!(
                record.schema_version,
                SOURCE_TRACKING_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | RESOLVED_METADATA_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | REMOVAL_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | PROJECTION_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | USER_PLUGIN_RECORD_SCHEMA_VERSION
            ) && matches!(record.agent_id.as_str(), "codex" | "claude-code")
                && Path::new(source).is_absolute()
                && resolved.as_ref().map_or(true, |resolved| {
                    source == resolved.stable_path.to_string_lossy().as_ref()
                })
                && record.linked_install_path.as_deref().map_or(true, |path| {
                    linked_record_cache_path_valid(workspace, record, Path::new(path))
                })
        }
    };
    let artifact_valid = if record.linked_source_path.is_some() {
        artifact_path.is_absolute()
            && resolved.as_ref().map_or(true, |resolved| {
                artifact_path == resolved.stable_path
                    && plugin_name(workspace, artifact_path, &record.install_id).is_ok()
            })
    } else {
        let artifact_digest = inspect_tree(artifact_path, ArtifactLimits::default())
            .and_then(|manifest| manifest.digest())
            .ok();
        artifact_path.is_absolute()
            && artifact_path.parent() == Some(state.user_plugin_artifacts().display_path())
            && artifact_path.file_name().and_then(|name| name.to_str())
                == record.artifact_digest.as_str().strip_prefix("sha256:")
            && artifact_digest.as_ref() == Some(&record.artifact_digest)
            && plugin_name(workspace, artifact_path, &record.install_id).is_ok()
    };
    let native_source_valid = if workspace.agent_id.as_str() == "codex" {
        std::fs::read_to_string(Path::new(&workspace.root_path).join("config.toml"))
            .ok()
            .and_then(|text| text.parse::<toml::Value>().ok())
            .and_then(|value| {
                value
                    .get("marketplaces")
                    .and_then(|marketplaces| marketplaces.get(&record.marketplace_name))
                    .cloned()
            })
            .map_or(true, |marketplace| {
                marketplace.get("source_type").and_then(toml::Value::as_str) == Some("local")
                    && marketplace
                        .get("source")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|source| {
                            Path::new(source) == Path::new(&record.marketplace_root)
                        })
            })
    } else {
        true
    };
    let valid = schema_valid
        && native_source_valid
        && (record.removal_phase.is_none()
            || matches!(
                record.schema_version,
                REMOVAL_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | PROJECTION_USER_PLUGIN_RECORD_SCHEMA_VERSION
                    | USER_PLUGIN_RECORD_SCHEMA_VERSION
            ))
        && record.cache_digest.as_ref().map_or(true, |_| {
            record.schema_version == USER_PLUGIN_RECORD_SCHEMA_VERSION
                && record.agent_id.as_str() == "codex"
                && record.linked_source_path.is_some()
                && record.linked_install_path.is_some()
        })
        && record.projection_entries.as_ref().map_or(true, |entries| {
            record.agent_id.as_str() == "codex"
                && !entries.is_empty()
                && entries.windows(2).all(|pair| pair[0] < pair[1])
                && entries.iter().all(|entry| {
                    Path::new(entry).components().count() == 1
                        && matches!(
                            Path::new(entry).components().next(),
                            Some(std::path::Component::Normal(_))
                        )
                })
        })
        && link_valid
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
        && artifact_valid
        && resolved.as_ref().map_or_else(
            || {
                missing_candidate.as_ref().is_some_and(|candidate| {
                    record.source_id == candidate.source_id
                        && record.install_id == candidate.install_id
                })
            },
            |resolved| {
                record.source_id == resolved.source_id && record.install_id == resolved.install_id
            },
        );
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

fn linked_record_cache_path_valid(
    workspace: &UserWorkspaceDescriptor,
    record: &UserPluginManagementRecord,
    installed_path: &Path,
) -> bool {
    let cache = Path::new(&workspace.root_path).join("plugins/cache");
    let Ok(relative) = installed_path.strip_prefix(cache) else {
        return false;
    };
    let components = relative.components().collect::<Vec<_>>();
    components.len() == 3
        && components[0].as_os_str() == record.marketplace_name.as_str()
        && components[1].as_os_str() == record.install_id.as_str()
        && matches!(components[2], std::path::Component::Normal(_))
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
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

    fn linked_plan(home: &Path, agent_id: &str, installed_path: &Path) -> StoredUserPluginPlan {
        let root = home.join(if agent_id == "codex" {
            ".codex"
        } else {
            ".claude"
        });
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let source = home.join("source-plugin");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "live source").unwrap();
        let artifact = home.join("snapshot-plugin");
        std::fs::create_dir_all(&artifact).unwrap();
        std::fs::write(artifact.join("README.md"), "snapshot").unwrap();
        let record = UserPluginManagementRecord {
            schema_version: USER_PLUGIN_RECORD_SCHEMA_VERSION,
            id: OwnershipRecordId::from("ownership:user-plugin-linked"),
            workspace_key: WorkspaceKey::from("workspace:user-linked"),
            installation_id: InstallationId::from(format!("{agent_id}:{}", root.to_string_lossy())),
            agent_id: AgentId::from(agent_id),
            resource_id: "catalog:linked".into(),
            source_id: "source:linked".into(),
            install_id: "demo".into(),
            native_id: "demo@ad-test".into(),
            marketplace_name: "ad-test".into(),
            marketplace_root: home
                .join(".ad/state/user-plugin-marketplaces/ad-test")
                .to_string_lossy()
                .into_owned(),
            artifact_path: artifact.to_string_lossy().into_owned(),
            artifact_digest: ContentDigest::sha256(b"snapshot"),
            tracking_policy: ResourceTrackingPolicy::Latest,
            resolved_source_revision: None,
            resolved_content_digest: None,
            linked_source_path: Some(
                std::fs::canonicalize(source)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
            linked_install_path: Some(
                std::fs::canonicalize(installed_path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
            projection_entries: None,
            cache_digest: None,
            removal_phase: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        stored_plan(&root, record, NativePluginAction::Install)
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn codex_linked_install_replaces_only_the_reported_cache_directory() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".codex/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("copied.txt"), "copy").unwrap();
        let canonical_installed = std::fs::canonicalize(&installed).unwrap();
        let plan = linked_plan(temp.path(), "codex", &installed);
        let output = CommandOutput {
            success: true,
            stdout: serde_json::json!({
                "pluginId": "demo@ad-test",
                "installedPath": installed,
            })
            .to_string(),
            stderr: String::new(),
        };

        let linked = link_installed_plugin(&plan, &output).unwrap();

        assert_eq!(linked.path, canonical_installed.to_string_lossy());
        assert!(installed.is_dir() && !installed.is_symlink());
        assert!(!installed.join("README.md").is_symlink());
        assert!(installed.join("README.md").is_file());
        let outside = temp.path().join("outside/demo/1.0.0");
        std::fs::create_dir_all(&outside).unwrap();
        let hostile = CommandOutput {
            success: true,
            stdout: serde_json::json!({"installedPath": outside}).to_string(),
            stderr: String::new(),
        };
        assert_eq!(
            installed_plugin_path(&plan, &hostile).unwrap_err().code,
            AgentErrorCode::PermissionDenied
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn codex_source_tracking_preserves_symlinks_without_following_external_targets() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".codex/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let installed = std::fs::canonicalize(installed).unwrap();
        std::fs::write(installed.join("native.txt"), "native cache").unwrap();
        let plan = linked_plan(temp.path(), "codex", &installed);
        let source = Path::new(plan.record.linked_source_path.as_deref().unwrap());
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel");
        std::fs::write(&sentinel, "external content").unwrap();
        std::fs::create_dir_all(source.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(source.join(".git")).unwrap();
        std::fs::write(source.join(".git/config"), "repository metadata").unwrap();
        let writable = source.join("writable.lock");
        std::fs::write(&writable, "lock").unwrap();
        std::fs::set_permissions(&writable, std::fs::Permissions::from_mode(0o666)).unwrap();
        let links = [
            (".agents/skills/external", outside.clone()),
            ("python", sentinel.clone()),
            ("relative", PathBuf::from("../outside")),
            ("broken", outside.join("missing")),
            ("cycle", PathBuf::from(".")),
        ];
        for (name, destination) in &links {
            symlink(destination, source.join(name)).unwrap();
        }
        let source_before = super::super::directory_tree_digest(source).unwrap();
        let output = CommandOutput {
            success: true,
            stdout: serde_json::json!({"installedPath": installed}).to_string(),
            stderr: String::new(),
        };

        let binding = link_installed_plugin(&plan, &output).unwrap();

        let target = linked_cache_target(&plan, &installed).unwrap();
        assert_eq!(target.observe().unwrap().digest(), binding.cache_digest);
        assert!(!installed.join(".git").exists());
        assert_eq!(
            std::fs::metadata(installed.join("writable.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o666
        );
        for (name, destination) in &links {
            assert_eq!(
                std::fs::read_link(installed.join(name)).unwrap(),
                *destination
            );
        }
        // External content is not copied into the digest or touched by sync/removal.
        std::fs::write(&sentinel, "changed externally").unwrap();
        assert_eq!(
            super::super::execution_tree::tracked_source_digest(source).unwrap(),
            binding.cache_digest.clone().unwrap()
        );
        std::fs::write(source.join("README.md"), "next source revision").unwrap();
        assert!(target
            .write_tracked_source_atomic(source, binding.cache_digest.as_ref().unwrap())
            .is_err());
        assert_eq!(
            std::fs::read_to_string(installed.join("README.md")).unwrap(),
            "live source"
        );
        let next = super::super::execution_tree::tracked_source_digest(source).unwrap();
        target.write_tracked_source_atomic(source, &next).unwrap();
        assert_eq!(
            std::fs::read_to_string(installed.join("README.md")).unwrap(),
            "next source revision"
        );
        // Generic directory publication retains its containment restriction.
        assert!(target
            .write_directory_atomic_filtered(source, false)
            .is_err());
        target.remove().unwrap();
        assert_eq!(
            std::fs::read_to_string(&sentinel).unwrap(),
            "changed externally"
        );
        std::fs::write(source.join("README.md"), "live source").unwrap();
        assert_eq!(
            super::super::directory_tree_digest(source).unwrap(),
            source_before
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn codex_initial_publication_rejects_source_version_drift() {
        for source_version in [Some("1.8.8"), None] {
            let temp = tempfile::tempdir().unwrap();
            let _guard = HomeGuard::set(temp.path());
            let installed = temp.path().join(".codex/plugins/cache/ad-test/demo/1.8.7");
            std::fs::create_dir_all(installed.join(".codex-plugin")).unwrap();
            let previous = serde_json::json!({"name": "demo", "version": "1.8.7"});
            std::fs::write(
                installed.join(".codex-plugin/plugin.json"),
                previous.to_string(),
            )
            .unwrap();
            std::fs::write(installed.join("copied.txt"), "native installed bytes").unwrap();
            let plan = linked_plan(temp.path(), "codex", &installed);
            let source = Path::new(plan.record.linked_source_path.as_deref().unwrap());
            std::fs::create_dir_all(source.join(".codex-plugin")).unwrap();
            let mut changed = serde_json::json!({"name": "demo"});
            if let Some(version) = source_version {
                changed["version"] = serde_json::json!(version);
            }
            std::fs::write(
                source.join(".codex-plugin/plugin.json"),
                changed.to_string(),
            )
            .unwrap();
            let output = CommandOutput {
                success: true,
                stdout: serde_json::json!({"installedPath": installed}).to_string(),
                stderr: String::new(),
            };

            let error = link_installed_plugin(&plan, &output).err().unwrap();

            assert_eq!(error.code, AgentErrorCode::ResourceChanged);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(
                    &std::fs::read(installed.join(".codex-plugin/plugin.json")).unwrap()
                )
                .unwrap(),
                previous
            );
            assert_eq!(
                std::fs::read_to_string(installed.join("copied.txt")).unwrap(),
                "native installed bytes"
            );
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn claude_linked_install_uses_the_registered_user_cache_path() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".claude/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let plan = linked_plan(temp.path(), "claude-code", &installed);
        let manifest = temp.path().join(".claude/plugins/installed_plugins.json");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 2,
                "plugins": {
                    "demo@ad-test": [{
                        "scope": "user",
                        "installPath": installed,
                        "version": "1.0.0"
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        link_installed_plugin(
            &plan,
            &CommandOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_link(&installed).unwrap(),
            std::fs::canonicalize(plan.record.linked_source_path.as_deref().unwrap()).unwrap()
        );
        assert!(remove_linked_plugin_cache(&plan, false).unwrap());
        assert!(!installed.exists());
        restore_linked_plugin_cache(
            &plan,
            Path::new(plan.record.linked_install_path.as_deref().unwrap()),
            true,
        )
        .unwrap();
        assert!(installed.is_symlink());
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn linked_uninstall_rejects_replaced_cache_targets_before_running_cli() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".codex/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let mut plan = linked_plan(temp.path(), "codex", &installed);
        plan.action = NativePluginAction::Remove;
        std::fs::remove_dir(&installed).unwrap();
        symlink(
            plan.record.linked_source_path.as_deref().unwrap(),
            &installed,
        )
        .unwrap();
        assert!(remove_linked_plugin_cache(&plan, false).unwrap());

        let missing_runner = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::new()),
        };
        plan.record.removal_phase = None;
        assert_eq!(
            execute_plan(&plan, &missing_runner).await.unwrap_err().code,
            AgentErrorCode::PermissionDenied
        );
        assert!(missing_runner.calls.lock().unwrap().is_empty());

        std::fs::create_dir_all(&installed).unwrap();
        let runner = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::new()),
        };
        assert_eq!(
            execute_plan(&plan, &runner).await.unwrap_err().code,
            AgentErrorCode::PermissionDenied
        );
        assert!(runner.calls.lock().unwrap().is_empty());
        std::fs::remove_dir(&installed).unwrap();
        let other = temp.path().join("other-source");
        std::fs::create_dir_all(&other).unwrap();
        symlink(&other, &installed).unwrap();
        assert_eq!(
            remove_linked_plugin_cache(&plan, false).unwrap_err().code,
            AgentErrorCode::PermissionDenied
        );
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn linked_uninstall_resumes_after_native_removal() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".claude/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let mut plan = linked_plan(temp.path(), "claude-code", &installed);
        plan.action = NativePluginAction::Remove;
        plan.record_preexisting = true;
        std::fs::remove_dir(&installed).unwrap();
        symlink(
            plan.record.linked_source_path.as_deref().unwrap(),
            &installed,
        )
        .unwrap();
        write_record(&plan.record).unwrap();
        let first = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([
                command_output(true, ""),
                command_output(false, "marketplace failed"),
            ])),
        };

        assert!(execute_plan(&plan, &first).await.is_err());
        assert_eq!(first.calls.lock().unwrap().len(), 2);
        assert!(std::fs::symlink_metadata(&installed).is_err());
        let state = ExecutionState::open().unwrap();
        let bytes = state
            .user_plugin_management()
            .read(&record_file_name(&plan.record.id))
            .unwrap();
        let persisted: UserPluginManagementRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            persisted.removal_phase,
            Some(UserPluginRemovalPhase::NativeRemoved)
        );

        let retry = stored_plan(
            Path::new(&plan.root_path),
            persisted,
            NativePluginAction::Remove,
        );
        let second = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([command_output(true, "")])),
        };
        execute_plan(&retry, &second).await.unwrap();
        assert_eq!(second.calls.lock().unwrap().len(), 1);
        assert!(state
            .user_plugin_management()
            .read(&record_file_name(&plan.record.id))
            .is_err());
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn failed_native_uninstall_restores_link_and_clears_progress() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".claude/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let mut plan = linked_plan(temp.path(), "claude-code", &installed);
        plan.action = NativePluginAction::Remove;
        std::fs::remove_dir(&installed).unwrap();
        symlink(
            plan.record.linked_source_path.as_deref().unwrap(),
            &installed,
        )
        .unwrap();
        write_record(&plan.record).unwrap();
        let runner = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([command_output(false, "native failed")])),
        };

        assert!(execute_plan(&plan, &runner).await.is_err());
        assert_eq!(
            std::fs::read_link(&installed).unwrap(),
            Path::new(plan.record.linked_source_path.as_deref().unwrap())
        );
        let state = ExecutionState::open().unwrap();
        let bytes = state
            .user_plugin_management()
            .read(&record_file_name(&plan.record.id))
            .unwrap();
        let persisted: UserPluginManagementRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(persisted.removal_phase, None);
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn failed_codex_uninstall_restores_exact_cache_without_reading_source() {
        for source_missing in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let _guard = HomeGuard::set(temp.path());
            let installed = temp.path().join(".codex/plugins/cache/ad-test/demo/1.0.0");
            std::fs::create_dir_all(&installed).unwrap();
            std::fs::write(installed.join("README.md"), "last synchronized bytes").unwrap();
            let mut plan = linked_plan(temp.path(), "codex", &installed);
            plan.action = NativePluginAction::Remove;
            plan.record.cache_digest =
                Some(super::super::directory_tree_digest(&installed).unwrap());
            let source = Path::new(plan.record.linked_source_path.as_deref().unwrap());
            if source_missing {
                std::fs::remove_dir_all(source).unwrap();
            } else {
                std::fs::write(source.join("README.md"), "unsynchronized source bytes").unwrap();
            }
            // Config parsing models the unregister failure after the cache was removed.
            std::fs::write(Path::new(&plan.root_path).join("config.toml"), "[invalid").unwrap();
            if source_missing {
                preserve_owned_codex_cache(&plan, false).unwrap();
                plan.record.removal_phase = Some(UserPluginRemovalPhase::LinkRemoved);
                remove_linked_plugin_cache(&plan, false).unwrap();
            }
            write_record(&plan.record).unwrap();
            let runner = ScriptedRunner {
                calls: Mutex::new(Vec::new()),
                outputs: Mutex::new(VecDeque::new()),
            };
            assert!(execute_plan(&plan, &runner).await.is_err());
            assert_eq!(
                std::fs::read_to_string(installed.join("README.md")).unwrap(),
                "last synchronized bytes"
            );
            assert_eq!(
                Some(super::super::directory_tree_digest(&installed).unwrap()),
                plan.record.cache_digest
            );
            let state = ExecutionState::open().unwrap();
            let persisted: UserPluginManagementRecord = serde_json::from_slice(
                &state
                    .user_plugin_management()
                    .read(&record_file_name(&plan.record.id))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(persisted.removal_phase, None);
            assert_eq!(persisted.cache_digest, plan.record.cache_digest);
            assert!(runner.calls.lock().unwrap().is_empty());
            assert!(state
                .user_plugin_artifacts()
                .open_directory(&codex_removal_backup_name(&plan))
                .is_err());
            std::fs::write(
                Path::new(&plan.root_path).join("config.toml"),
                "[plugins.\"demo@ad-test\"]\nenabled = true\n",
            )
            .unwrap();
            let retry = stored_plan(
                Path::new(&plan.root_path),
                persisted,
                NativePluginAction::Remove,
            );
            execute_plan(&retry, &runner).await.unwrap();
            assert!(!installed.exists());
            assert!(state
                .user_plugin_management()
                .read(&record_file_name(&plan.record.id))
                .is_err());
        }
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn interrupted_link_removal_restores_on_later_native_failure() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let installed = temp.path().join(".claude/plugins/cache/ad-test/demo/1.0.0");
        std::fs::create_dir_all(&installed).unwrap();
        let mut plan = linked_plan(temp.path(), "claude-code", &installed);
        plan.action = NativePluginAction::Remove;
        plan.record.removal_phase = Some(UserPluginRemovalPhase::LinkRemovalPending);
        std::fs::remove_dir(&installed).unwrap();
        write_record(&plan.record).unwrap();
        let runner = ScriptedRunner {
            calls: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([command_output(false, "native failed")])),
        };

        assert!(execute_plan(&plan, &runner).await.is_err());
        assert_eq!(
            std::fs::read_link(&installed).unwrap(),
            Path::new(plan.record.linked_source_path.as_deref().unwrap())
        );
        let state = ExecutionState::open().unwrap();
        let bytes = state
            .user_plugin_management()
            .read(&record_file_name(&plan.record.id))
            .unwrap();
        let persisted: UserPluginManagementRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(persisted.removal_phase, None);
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
            tracking_policy: ResourceTrackingPolicy::Latest,
            resolved_source_revision: None,
            resolved_content_digest: None,
            linked_source_path: None,
            linked_install_path: None,
            projection_entries: None,
            cache_digest: None,
            removal_phase: None,
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

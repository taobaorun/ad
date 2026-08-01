use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::fs::paths::{
    project_skills_dir, skill_artifacts_dir, skill_library_dir, skill_migration_archive_dir,
    skill_sources_path, state_dir,
};
use crate::models::{ProjectSkillConfig, SkillListMode, SkillSource, SkillSourceType};

use super::execution_state::ExecutionState;
use super::skill_artifact_tree::{inspect_tree, ArtifactLimits};
use super::{
    decode_operation_receipt, ownership_record_id, validate_ownership_artifact,
    validate_ownership_record, ContentDigest, OperationStatus, ReceiptId, ResourceKind,
    ResourceOwnershipChangeKind, ResourceOwnershipRecord, ResourceScope, ResourceStateKind,
    OWNERSHIP_EVIDENCE_VERSION,
};

const LEGACY_INVENTORY_SCHEMA_VERSION: u32 = 1;
const MAX_LEGACY_STATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
struct LegacySkillSourceIndex {
    source_id: String,
    skills: BTreeMap<String, ContentDigest>,
}

type LegacySkillSourceInspection = (
    Vec<LegacySkillSourceView>,
    Vec<LegacySkillSourceIndex>,
    Vec<LegacySkillDiagnostic>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyManagedLinkEvidence {
    pub(crate) source_id: String,
    pub(crate) ownership_record: ResourceOwnershipRecord,
    pub(crate) receipt_id: ReceiptId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillInventory {
    pub schema_version: u32,
    pub sources: Vec<LegacySkillSourceView>,
    pub projects: Vec<LegacyProjectSkillView>,
    pub archives: Vec<LegacyProjectSkillArchiveView>,
    pub diagnostics: Vec<LegacySkillDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillSourceView {
    pub legacy_id: String,
    pub source_type: SkillSourceType,
    pub display_location: String,
    pub safe_identity: bool,
    pub available: bool,
    pub health: LegacySkillHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProjectSkillView {
    pub state_id: String,
    pub state_digest: ContentDigest,
    pub project_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_project_path: Option<String>,
    pub online: bool,
    pub mode: SkillListMode,
    pub listed_skills: Vec<String>,
    pub missing_source_ids: Vec<String>,
    pub links: Vec<LegacySkillLinkView>,
    pub health: LegacySkillHealth,
    pub migration_status: LegacySkillMigrationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillLinkView {
    pub logical_id: String,
    pub target_kind: LegacySkillLinkTargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_receipt_id: Option<ReceiptId>,
    pub health: LegacySkillHealth,
    #[serde(skip)]
    pub(crate) managed_evidence: Option<LegacyManagedLinkEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillLinkTargetKind {
    ManagedArtifact,
    LegacyCheckout,
    LocalSource,
    External,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillMigrationStatus {
    ReadyToArchive,
    NeedsReconciliation,
    Blocked,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillArchiveStatus {
    Archived,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProjectSkillArchiveView {
    pub schema_version: u32,
    pub archive_id: String,
    pub original_state_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub state_digest: ContentDigest,
    pub receipt_id: ReceiptId,
    pub archived_at: DateTime<Utc>,
    pub status: LegacySkillArchiveStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacySkillArchiveMarker {
    pub schema_version: u32,
    pub archive_id: String,
    pub archive_name: String,
    pub original_state_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub state_digest: ContentDigest,
    pub receipt_id: ReceiptId,
    pub archived_at: DateTime<Utc>,
    pub status: LegacySkillArchiveStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillHealth {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillDiagnostic {
    pub code: String,
    pub message_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LegacySkillInventoryError {
    #[error("legacy Skill inventory I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("legacy Skill inventory path failed: {0}")]
    Path(String),
}

pub fn inspect_legacy_skill_inventory() -> Result<LegacySkillInventory, LegacySkillInventoryError> {
    let (sources, source_index, mut diagnostics) = inspect_sources()?;
    let (mut projects, project_diagnostics) = inspect_projects(&sources, &source_index)?;
    diagnostics.extend(project_diagnostics);
    mark_project_aliases(&mut projects, &mut diagnostics);
    let (archives, archive_diagnostics) = inspect_archives()?;
    diagnostics.extend(archive_diagnostics);
    Ok(LegacySkillInventory {
        schema_version: LEGACY_INVENTORY_SCHEMA_VERSION,
        sources,
        projects,
        archives,
        diagnostics,
    })
}

fn inspect_sources() -> Result<LegacySkillSourceInspection, LegacySkillInventoryError> {
    let path =
        skill_sources_path().map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    if !path.exists() {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let bytes = read_limited(&path)?;
    let parsed = serde_json::from_slice::<Vec<SkillSource>>(&bytes);
    let Ok(mut sources) = parsed else {
        return Ok((
            Vec::new(),
            Vec::new(),
            vec![diagnostic("legacy_source_registry_invalid", None)],
        ));
    };
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    let mut views = Vec::new();
    let mut source_index = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    for source in sources {
        let safe_identity = safe_legacy_id(&source.id);
        let duplicate = !seen.insert(source.id.clone());
        let source_path = source_root(&source, safe_identity);
        let available = source_path.as_deref().is_some_and(Path::is_dir);
        let mut health = if !safe_identity || duplicate {
            LegacySkillHealth::Blocked
        } else if available {
            LegacySkillHealth::Ready
        } else {
            LegacySkillHealth::Degraded
        };
        if !safe_identity {
            diagnostics.push(diagnostic(
                "legacy_source_id_unsafe",
                Some(source.id.clone()),
            ));
        }
        if duplicate {
            diagnostics.push(diagnostic(
                "legacy_source_duplicate",
                Some(source.id.clone()),
            ));
        }
        if safe_identity && !available {
            diagnostics.push(diagnostic("legacy_source_missing", Some(source.id.clone())));
        }
        if safe_identity && !duplicate && available {
            match inspect_source_skill_index(&source) {
                Ok(skills) => source_index.push(LegacySkillSourceIndex {
                    source_id: source.id.clone(),
                    skills,
                }),
                Err(_) => {
                    health = LegacySkillHealth::Degraded;
                    diagnostics.push(diagnostic(
                        "legacy_source_inventory_incomplete",
                        Some(source.id.clone()),
                    ));
                }
            }
        }
        let display_location = display_location(&source);
        views.push(LegacySkillSourceView {
            legacy_id: source.id,
            source_type: source.source_type,
            display_location,
            safe_identity,
            available,
            health,
        });
    }
    Ok((views, source_index, diagnostics))
}

fn inspect_projects(
    sources: &[LegacySkillSourceView],
    source_index: &[LegacySkillSourceIndex],
) -> Result<(Vec<LegacyProjectSkillView>, Vec<LegacySkillDiagnostic>), LegacySkillInventoryError> {
    let root =
        project_skills_dir().map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    if !root.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let known_sources = sources
        .iter()
        .map(|source| source.legacy_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut entries = std::fs::read_dir(&root)
        .map_err(|source| io_error(&root, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io_error(&root, source))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut projects = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in entries {
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|source| io_error(&path, source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            diagnostics.push(diagnostic(
                "legacy_project_state_not_file",
                Some(entry.file_name().to_string_lossy().into_owned()),
            ));
            continue;
        }
        let state_id = entry.file_name().to_string_lossy().into_owned();
        let bytes = match read_limited(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                diagnostics.push(diagnostic(
                    "legacy_project_state_unreadable",
                    Some(state_id),
                ));
                continue;
            }
        };
        let Ok(config) = serde_json::from_slice::<ProjectSkillConfig>(&bytes) else {
            diagnostics.push(diagnostic("legacy_project_state_invalid", Some(state_id)));
            continue;
        };
        let state_digest = ContentDigest::sha256(&bytes);
        let project = Path::new(&config.project_path);
        let canonical = std::fs::canonicalize(project)
            .ok()
            .filter(|path| path.is_dir());
        let online = canonical.is_some();
        let canonical_project_path = canonical
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let missing_source_ids = config
            .listed_skills
            .iter()
            .filter_map(|skill| skill.split_once('/').map(|(source, _)| source))
            .filter(|source| !known_sources.contains(source))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if !online {
            diagnostics.push(diagnostic(
                "legacy_project_offline",
                Some(config.project_path.clone()),
            ));
        }
        if !missing_source_ids.is_empty() {
            diagnostics.push(diagnostic(
                "legacy_project_source_missing",
                Some(config.project_path.clone()),
            ));
        }
        let ownership = canonical
            .as_deref()
            .map(load_project_ownership)
            .transpose()?
            .unwrap_or_default();
        let links = canonical
            .as_deref()
            .map(|project| inspect_project_links(project, sources, source_index, &ownership))
            .transpose()?
            .unwrap_or_default();
        let (migration_status, intent_diagnostics) =
            migration_status(&config, online, &missing_source_ids, source_index, &links);
        diagnostics.extend(intent_diagnostics);
        let health = match migration_status {
            LegacySkillMigrationStatus::ReadyToArchive => LegacySkillHealth::Ready,
            LegacySkillMigrationStatus::NeedsReconciliation
            | LegacySkillMigrationStatus::Offline => LegacySkillHealth::Degraded,
            LegacySkillMigrationStatus::Blocked => LegacySkillHealth::Blocked,
        };
        projects.push(LegacyProjectSkillView {
            state_id,
            state_digest,
            project_path: config.project_path,
            canonical_project_path,
            online,
            mode: config.mode,
            listed_skills: config.listed_skills,
            missing_source_ids,
            links,
            health,
            migration_status,
        });
    }
    Ok((projects, diagnostics))
}

fn inspect_project_links(
    project: &Path,
    sources: &[LegacySkillSourceView],
    source_index: &[LegacySkillSourceIndex],
    ownership: &[ResourceOwnershipRecord],
) -> Result<Vec<LegacySkillLinkView>, LegacySkillInventoryError> {
    let claude_root = project.join(".claude");
    let root = project.join(".claude/skills");
    let claude_metadata = match std::fs::symlink_metadata(&claude_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&claude_root, error)),
    };
    let root_metadata = match std::fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&root, error)),
    };
    if !claude_metadata.is_dir()
        || claude_metadata.file_type().is_symlink()
        || !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
    {
        return Ok(vec![LegacySkillLinkView {
            logical_id: "<skills-root>".into(),
            target_kind: LegacySkillLinkTargetKind::External,
            source_id: None,
            migration_receipt_id: None,
            health: LegacySkillHealth::Blocked,
            managed_evidence: None,
        }]);
    }
    let library =
        skill_library_dir().map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    let canonical_library = std::fs::canonicalize(&library).ok();
    let artifacts = skill_artifacts_dir()
        .map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    let canonical_artifacts = std::fs::canonicalize(&artifacts).ok();
    let local_sources = sources
        .iter()
        .filter(|source| source.source_type == SkillSourceType::Local && source.available)
        .filter_map(|source| {
            std::fs::canonicalize(&source.display_location)
                .ok()
                .map(|path| (source.legacy_id.as_str(), path))
        })
        .collect::<Vec<_>>();
    let mut links = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|source| io_error(&root, source))? {
        let entry = entry.map_err(|source| io_error(&root, source))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|source| io_error(&entry.path(), source))?;
        let logical_id = entry.file_name().to_string_lossy().into_owned();
        if !metadata.file_type().is_symlink() {
            links.push(LegacySkillLinkView {
                logical_id,
                target_kind: LegacySkillLinkTargetKind::External,
                source_id: None,
                migration_receipt_id: None,
                health: LegacySkillHealth::Blocked,
                managed_evidence: None,
            });
            continue;
        }
        if let Some(evidence) =
            managed_link_evidence(&entry.path(), &logical_id, ownership, source_index)
        {
            links.push(LegacySkillLinkView {
                logical_id,
                target_kind: LegacySkillLinkTargetKind::ManagedArtifact,
                source_id: Some(evidence.source_id.clone()),
                migration_receipt_id: Some(evidence.receipt_id.clone()),
                health: LegacySkillHealth::Ready,
                managed_evidence: Some(evidence),
            });
            continue;
        }
        let target = std::fs::canonicalize(entry.path());
        let (target_kind, source_id, health) = match target {
            Err(_) => (
                LegacySkillLinkTargetKind::Missing,
                None,
                LegacySkillHealth::Degraded,
            ),
            Ok(target)
                if canonical_artifacts
                    .as_ref()
                    .is_some_and(|artifacts| target.starts_with(artifacts)) =>
            {
                (
                    LegacySkillLinkTargetKind::ManagedArtifact,
                    matching_source_id(source_index, &logical_id, &target),
                    LegacySkillHealth::Degraded,
                )
            }
            Ok(target)
                if canonical_library
                    .as_ref()
                    .is_some_and(|library| target.starts_with(library)) =>
            {
                let source_id = target
                    .strip_prefix(canonical_library.as_ref().expect("guarded above"))
                    .ok()
                    .and_then(|path| path.components().next())
                    .and_then(|component| component.as_os_str().to_str())
                    .map(str::to_owned);
                (
                    LegacySkillLinkTargetKind::LegacyCheckout,
                    source_id,
                    LegacySkillHealth::Ready,
                )
            }
            Ok(target) => match local_sources
                .iter()
                .find(|(_, source)| target.starts_with(source))
            {
                Some((source_id, _)) => (
                    LegacySkillLinkTargetKind::LocalSource,
                    Some((*source_id).to_owned()),
                    LegacySkillHealth::Ready,
                ),
                None => (
                    LegacySkillLinkTargetKind::External,
                    None,
                    LegacySkillHealth::Blocked,
                ),
            },
        };
        links.push(LegacySkillLinkView {
            logical_id,
            target_kind,
            source_id,
            migration_receipt_id: None,
            health,
            managed_evidence: None,
        });
    }
    links.sort_by(|left, right| left.logical_id.cmp(&right.logical_id));
    Ok(links)
}

fn mark_project_aliases(
    projects: &mut [LegacyProjectSkillView],
    diagnostics: &mut Vec<LegacySkillDiagnostic>,
) {
    let mut canonical_counts = BTreeMap::<String, usize>::new();
    for project in projects.iter() {
        if let Some(canonical) = project.canonical_project_path.as_ref() {
            *canonical_counts.entry(canonical.clone()).or_default() += 1;
        }
    }
    for project in projects {
        let duplicate_alias = project
            .canonical_project_path
            .as_ref()
            .and_then(|canonical| canonical_counts.get(canonical))
            .is_some_and(|count| *count > 1);
        let embedded_alias = project
            .canonical_project_path
            .as_deref()
            .is_some_and(|canonical| canonical != project.project_path);
        let alias = duplicate_alias || embedded_alias;
        let expected_state = format!(
            "{}.json",
            crate::commands::apply::slug_from_path(&project.project_path)
        );
        let mismatched_slug = project.state_id != expected_state;
        if alias || mismatched_slug {
            project.health = LegacySkillHealth::Blocked;
            project.migration_status = LegacySkillMigrationStatus::Blocked;
            diagnostics.push(diagnostic(
                if alias {
                    "legacy_project_path_alias"
                } else {
                    "legacy_project_slug_mismatch"
                },
                Some(project.project_path.clone()),
            ));
        }
    }
}

fn migration_status(
    config: &ProjectSkillConfig,
    online: bool,
    missing_source_ids: &[String],
    source_index: &[LegacySkillSourceIndex],
    links: &[LegacySkillLinkView],
) -> (LegacySkillMigrationStatus, Vec<LegacySkillDiagnostic>) {
    if !online {
        return (LegacySkillMigrationStatus::Offline, Vec::new());
    }
    let mut diagnostics = Vec::new();
    let available_skills = source_index
        .iter()
        .flat_map(|source| {
            source
                .skills
                .keys()
                .map(|name| (source.source_id.clone(), name.clone()))
        })
        .collect::<BTreeSet<_>>();
    let mut listed_skills = BTreeSet::new();
    let mut listed_names = BTreeSet::new();
    let mut invalid_intent = false;
    for listed in &config.listed_skills {
        let Some((source, name)) = listed.split_once('/') else {
            invalid_intent = true;
            continue;
        };
        let identity = (source.to_owned(), name.to_owned());
        if source.is_empty()
            || name.is_empty()
            || name.contains('/')
            || !listed_skills.insert(identity.clone())
            || !listed_names.insert(name)
            || !available_skills.contains(&identity)
        {
            invalid_intent = true;
        }
    }
    if invalid_intent {
        diagnostics.push(diagnostic(
            "legacy_project_intent_ambiguous",
            Some(config.project_path.clone()),
        ));
    }
    let managed = links
        .iter()
        .filter_map(|link| {
            link.source_id
                .as_ref()
                .map(|source| (source.clone(), link.logical_id.clone()))
        })
        .collect::<BTreeSet<_>>();
    let intent_drift = match config.mode {
        SkillListMode::Allowlist => listed_skills != managed,
        SkillListMode::Blocklist => {
            let enabled = available_skills
                .difference(&listed_skills)
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut enabled_names = BTreeSet::new();
            let duplicate_enabled_name =
                enabled.iter().any(|(_, name)| !enabled_names.insert(name));
            !listed_skills.is_empty()
                || duplicate_enabled_name
                || enabled.iter().any(|skill| !managed.contains(skill))
        }
    };
    if intent_drift {
        diagnostics.push(diagnostic(
            "legacy_project_intent_drift",
            Some(config.project_path.clone()),
        ));
    }
    if invalid_intent
        || !missing_source_ids.is_empty()
        || links
            .iter()
            .any(|link| link.target_kind == LegacySkillLinkTargetKind::External)
    {
        return (LegacySkillMigrationStatus::Blocked, diagnostics);
    }
    if intent_drift
        || links.iter().any(|link| {
            link.target_kind != LegacySkillLinkTargetKind::ManagedArtifact
                || link.managed_evidence.is_none()
        })
    {
        return (LegacySkillMigrationStatus::NeedsReconciliation, diagnostics);
    }
    (LegacySkillMigrationStatus::ReadyToArchive, diagnostics)
}

fn load_project_ownership(
    canonical_project: &Path,
) -> Result<Vec<ResourceOwnershipRecord>, LegacySkillInventoryError> {
    let root = state_dir()
        .map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?
        .join("resource-ownership");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let canonical = canonical_project.to_string_lossy();
    let mut records = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|source| io_error(&root, source))? {
        let entry = entry.map_err(|source| io_error(&root, source))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|source| io_error(&entry.path(), source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let bytes = match read_limited(&entry.path()) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let Ok(record) = serde_json::from_slice::<ResourceOwnershipRecord>(&bytes) else {
            continue;
        };
        if record.resource.kind != ResourceKind::Skills
            || record.resource.scope != ResourceScope::Project
            || record.resource.project_path.as_deref() != Some(canonical.as_ref())
            || record.id != ownership_record_id(&record.resource)
        {
            continue;
        }
        records.push(record);
    }
    Ok(records)
}

fn managed_link_evidence(
    link: &Path,
    logical_id: &str,
    ownership: &[ResourceOwnershipRecord],
    source_index: &[LegacySkillSourceIndex],
) -> Option<LegacyManagedLinkEvidence> {
    let target = std::fs::read_link(link).ok()?;
    let digest = ContentDigest::sha256(target.to_string_lossy().as_bytes());
    let state = ExecutionState::open().ok()?;
    ownership.iter().find_map(|record| {
        let path_matches = record.target_path == link.to_string_lossy();
        let logical_id_matches = record.resource.logical_id.rsplit('/').next() == Some(logical_id);
        let record_valid = validate_ownership_record(
            record,
            &record.resource,
            link,
            ResourceStateKind::Symlink,
            Some(&digest),
        )
        .is_ok();
        let artifact_valid = validate_ownership_artifact(record).is_ok();
        let receipt_valid = operation_receipt_proves_record(&state, record, &digest);
        if !path_matches
            || !logical_id_matches
            || !record_valid
            || !artifact_valid
            || !receipt_valid
        {
            return None;
        }
        matching_source_id(source_index, logical_id, Path::new(&record.artifact_id)).map(
            |source_id| LegacyManagedLinkEvidence {
                source_id,
                ownership_record: record.clone(),
                receipt_id: record.updated_by_receipt_id.clone(),
            },
        )
    })
}

fn matching_source_id(
    source_index: &[LegacySkillSourceIndex],
    logical_id: &str,
    artifact: &Path,
) -> Option<String> {
    let artifact_identity = normalized_skill_digest(artifact).ok()?;
    let mut matches = source_index
        .iter()
        .filter(|source| source.skills.get(logical_id) == Some(&artifact_identity))
        .map(|source| source.source_id.clone());
    let source_id = matches.next()?;
    matches.next().is_none().then_some(source_id)
}

pub(super) fn operation_receipt_proves_record(
    state: &ExecutionState,
    record: &ResourceOwnershipRecord,
    target_digest: &ContentDigest,
) -> bool {
    let name = format!("{}.json", record.updated_by_receipt_id);
    let Ok(bytes) = state.history().read(&name) else {
        return false;
    };
    let Ok(receipt) = decode_operation_receipt(&bytes) else {
        return false;
    };
    receipt.id == record.updated_by_receipt_id
        && receipt.status == OperationStatus::Complete
        && receipt.workspace_key.as_ref() == Some(&record.workspace_key)
        && receipt.ownership_evidence_version == OWNERSHIP_EVIDENCE_VERSION
        && receipt.post_apply_states.iter().any(|state| {
            state.resource == record.resource
                && state.kind == ResourceStateKind::Symlink
                && state.digest.as_ref() == Some(target_digest)
        })
        && receipt.ownership_changes.iter().any(|change| {
            change.kind == ResourceOwnershipChangeKind::Upsert
                && change.record_id == record.id
                && change.record.as_ref() == Some(record)
        })
}

fn inspect_source_skill_index(
    source: &SkillSource,
) -> Result<BTreeMap<String, ContentDigest>, LegacySkillInventoryError> {
    let root = source_root(source, true).ok_or_else(|| {
        LegacySkillInventoryError::Path(format!("legacy source root is unavailable: {}", source.id))
    })?;
    let entries = std::fs::read_dir(&root)
        .map_err(|source_error| io_error(&root, source_error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source_error| io_error(&root, source_error))?;
    let mut skills = BTreeMap::new();
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !path.is_dir() || !path.join("SKILL.md").is_file() {
            continue;
        }
        let digest = normalized_skill_digest(&path)?;
        skills.insert(name, digest);
    }
    Ok(skills)
}

fn normalized_skill_digest(path: &Path) -> Result<ContentDigest, LegacySkillInventoryError> {
    inspect_tree(path, ArtifactLimits::default())
        .and_then(|manifest| manifest.digest())
        .map_err(|error| LegacySkillInventoryError::Path(error.to_string()))
}

fn inspect_archives() -> Result<
    (
        Vec<LegacyProjectSkillArchiveView>,
        Vec<LegacySkillDiagnostic>,
    ),
    LegacySkillInventoryError,
> {
    let root = skill_migration_archive_dir()
        .map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    if !root.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut archives = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|source| io_error(&root, source))? {
        let entry = entry.map_err(|source| io_error(&root, source))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".marker.json") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|source| io_error(&entry.path(), source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            diagnostics.push(diagnostic("legacy_archive_marker_invalid", Some(name)));
            continue;
        }
        let bytes = match read_limited(&entry.path()) {
            Ok(bytes) => bytes,
            Err(_) => {
                diagnostics.push(diagnostic("legacy_archive_marker_unreadable", Some(name)));
                continue;
            }
        };
        let Ok(marker) = serde_json::from_slice::<LegacySkillArchiveMarker>(&bytes) else {
            diagnostics.push(diagnostic("legacy_archive_marker_invalid", Some(name)));
            continue;
        };
        if marker.schema_version != LEGACY_INVENTORY_SCHEMA_VERSION {
            diagnostics.push(diagnostic("legacy_archive_marker_future", Some(name)));
            continue;
        }
        let expected_archive_name = format!(
            "{:x}.legacy.json",
            Sha256::digest(marker.archive_id.as_bytes())
        );
        let expected_marker_name = format!(
            "{:x}.marker.json",
            Sha256::digest(marker.archive_id.as_bytes())
        );
        if marker.archive_name != expected_archive_name || name != expected_marker_name {
            diagnostics.push(diagnostic(
                "legacy_archive_identity_invalid",
                Some(marker.archive_id),
            ));
            continue;
        }
        if marker.status == LegacySkillArchiveStatus::Archived {
            let archive = root.join(&marker.archive_name);
            let Ok(metadata) = std::fs::symlink_metadata(&archive) else {
                diagnostics.push(diagnostic(
                    "legacy_archive_bytes_unavailable",
                    Some(marker.archive_id),
                ));
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                diagnostics.push(diagnostic(
                    "legacy_archive_bytes_invalid",
                    Some(marker.archive_id),
                ));
                continue;
            }
            let Ok(archive_bytes) = read_limited(&archive) else {
                diagnostics.push(diagnostic(
                    "legacy_archive_bytes_unavailable",
                    Some(marker.archive_id),
                ));
                continue;
            };
            if ContentDigest::sha256(&archive_bytes) != marker.state_digest
                || serde_json::from_slice::<ProjectSkillConfig>(&archive_bytes).is_err()
            {
                diagnostics.push(diagnostic(
                    "legacy_archive_bytes_invalid",
                    Some(marker.archive_id),
                ));
                continue;
            }
        }
        archives.push(LegacyProjectSkillArchiveView {
            schema_version: marker.schema_version,
            archive_id: marker.archive_id,
            original_state_id: marker.original_state_id,
            project_path: marker.project_path,
            canonical_project_path: marker.canonical_project_path,
            state_digest: marker.state_digest,
            receipt_id: marker.receipt_id,
            archived_at: marker.archived_at,
            status: marker.status,
        });
    }
    archives.sort_by(|left, right| left.archive_id.cmp(&right.archive_id));
    Ok((archives, diagnostics))
}

fn source_root(source: &SkillSource, safe_identity: bool) -> Option<PathBuf> {
    let root = match source.source_type {
        SkillSourceType::Git if safe_identity => skill_library_dir().ok()?.join(&source.id),
        SkillSourceType::Git => return None,
        SkillSourceType::Local => PathBuf::from(&source.url),
    };
    Some(match source.subdirectory.as_deref() {
        Some(subdirectory) => root.join(subdirectory),
        None => root,
    })
}

fn safe_legacy_id(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && Path::new(value).components().count() == 1
}

fn display_location(source: &SkillSource) -> String {
    if source.source_type == SkillSourceType::Local {
        return source.url.clone();
    }
    let Ok(mut url) = url::Url::parse(&source.url) else {
        return source.url.clone();
    };
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    url.to_string()
}

fn read_limited(path: &Path) -> Result<Vec<u8>, LegacySkillInventoryError> {
    let metadata = std::fs::metadata(path).map_err(|source| io_error(path, source))?;
    if metadata.len() > MAX_LEGACY_STATE_BYTES {
        return Err(LegacySkillInventoryError::Io {
            path: path.display().to_string(),
            source: std::io::Error::other("legacy state exceeds the read budget"),
        });
    }
    std::fs::read(path).map_err(|source| io_error(path, source))
}

fn diagnostic(code: &str, subject_id: Option<String>) -> LegacySkillDiagnostic {
    LegacySkillDiagnostic {
        code: code.into(),
        message_key: format!("agents.skills.migration.{code}"),
        subject_id,
    }
}

fn io_error(path: &Path, source: std::io::Error) -> LegacySkillInventoryError {
    LegacySkillInventoryError::Io {
        path: path.display().to_string(),
        source,
    }
}

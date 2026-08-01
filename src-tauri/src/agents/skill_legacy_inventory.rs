use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs::paths::{project_skills_dir, skill_library_dir, skill_sources_path};
use crate::models::{ProjectSkillConfig, SkillListMode, SkillSource, SkillSourceType};

const LEGACY_INVENTORY_SCHEMA_VERSION: u32 = 1;
const MAX_LEGACY_STATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillInventory {
    pub schema_version: u32,
    pub sources: Vec<LegacySkillSourceView>,
    pub projects: Vec<LegacyProjectSkillView>,
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
    pub project_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_project_path: Option<String>,
    pub online: bool,
    pub mode: SkillListMode,
    pub listed_skills: Vec<String>,
    pub missing_source_ids: Vec<String>,
    pub links: Vec<LegacySkillLinkView>,
    pub health: LegacySkillHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySkillLinkView {
    pub logical_id: String,
    pub target_kind: LegacySkillLinkTargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub health: LegacySkillHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacySkillLinkTargetKind {
    LegacyCheckout,
    LocalSource,
    External,
    Missing,
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
    let (sources, mut diagnostics) = inspect_sources()?;
    let (mut projects, project_diagnostics) = inspect_projects(&sources)?;
    diagnostics.extend(project_diagnostics);
    mark_project_aliases(&mut projects, &mut diagnostics);
    Ok(LegacySkillInventory {
        schema_version: LEGACY_INVENTORY_SCHEMA_VERSION,
        sources,
        projects,
        diagnostics,
    })
}

fn inspect_sources(
) -> Result<(Vec<LegacySkillSourceView>, Vec<LegacySkillDiagnostic>), LegacySkillInventoryError> {
    let path =
        skill_sources_path().map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let bytes = read_limited(&path)?;
    let parsed = serde_json::from_slice::<Vec<SkillSource>>(&bytes);
    let Ok(mut sources) = parsed else {
        return Ok((
            Vec::new(),
            vec![diagnostic("legacy_source_registry_invalid", None)],
        ));
    };
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    let mut views = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    for source in sources {
        let safe_identity = safe_legacy_id(&source.id);
        let duplicate = !seen.insert(source.id.clone());
        let source_path = source_root(&source, safe_identity);
        let available = source_path.as_deref().is_some_and(Path::is_dir);
        let health = if !safe_identity || duplicate {
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
    Ok((views, diagnostics))
}

fn inspect_projects(
    sources: &[LegacySkillSourceView],
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
        let links = canonical
            .as_deref()
            .map(|project| inspect_project_links(project, sources))
            .transpose()?
            .unwrap_or_default();
        let health = if !online || !missing_source_ids.is_empty() {
            LegacySkillHealth::Degraded
        } else if links
            .iter()
            .any(|link| link.health == LegacySkillHealth::Blocked)
        {
            LegacySkillHealth::Blocked
        } else {
            LegacySkillHealth::Ready
        };
        projects.push(LegacyProjectSkillView {
            state_id,
            project_path: config.project_path,
            canonical_project_path,
            online,
            mode: config.mode,
            listed_skills: config.listed_skills,
            missing_source_ids,
            links,
            health,
        });
    }
    Ok((projects, diagnostics))
}

fn inspect_project_links(
    project: &Path,
    sources: &[LegacySkillSourceView],
) -> Result<Vec<LegacySkillLinkView>, LegacySkillInventoryError> {
    let root = project.join(".claude/skills");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let library =
        skill_library_dir().map_err(|error| LegacySkillInventoryError::Path(error.to_string()))?;
    let canonical_library = std::fs::canonicalize(&library).ok();
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
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let logical_id = entry.file_name().to_string_lossy().into_owned();
        let target = std::fs::canonicalize(entry.path());
        let (target_kind, source_id, health) = match target {
            Err(_) => (
                LegacySkillLinkTargetKind::Missing,
                None,
                LegacySkillHealth::Degraded,
            ),
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
            health,
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
        let alias = project
            .canonical_project_path
            .as_ref()
            .and_then(|canonical| canonical_counts.get(canonical))
            .is_some_and(|count| *count > 1);
        let expected_state = format!(
            "{}.json",
            crate::commands::apply::slug_from_path(&project.project_path)
        );
        let mismatched_slug = project.state_id != expected_state;
        if alias || mismatched_slug {
            project.health = LegacySkillHealth::Blocked;
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

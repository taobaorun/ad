use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::fs::paths::{claude_skills_dir, skill_library_dir, skill_sources_path};
use crate::models::{SkillEntry, SkillEntrySource, SkillScope, SkillSource, SkillSourceType};

#[derive(Debug, thiserror::Error)]
pub(crate) enum LegacySkillDiscoveryError {
    #[error("legacy Skill discovery path failed: {0}")]
    Path(String),
    #[error("legacy Skill discovery I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("legacy Skill source registry is invalid: {0}")]
    InvalidRegistry(#[from] serde_json::Error),
}

pub(crate) fn scan_legacy_skill_library_read_only(
    project_path: Option<String>,
) -> Result<Vec<SkillEntry>, LegacySkillDiscoveryError> {
    let mut sources = load_sources()?;
    discover_unregistered_sources(&mut sources)?;
    let claude_skills =
        claude_skills_dir().map_err(|error| LegacySkillDiscoveryError::Path(error.to_string()))?;
    let mut entries = Vec::new();
    for source in &sources {
        let Some(root) = skills_root_for(source) else {
            continue;
        };
        if !root.is_dir() {
            continue;
        }
        let version = git_version_for(source);
        let mut skill_dirs = Vec::new();
        find_skill_dirs(&root, 4, &mut skill_dirs);
        for skill_path in skill_dirs {
            let name = skill_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            entries.push(SkillEntry {
                name: name.clone(),
                path: skill_path.to_string_lossy().into_owned(),
                source: SkillEntrySource::Managed,
                source_id: Some(source.id.clone()),
                version: version.clone(),
                description: parse_skill_description(&skill_path),
                scope: determine_scope(&name, &claude_skills, project_path.as_deref()),
            });
        }
    }
    if claude_skills.is_dir() {
        for entry in
            std::fs::read_dir(&claude_skills).map_err(|source| io_error(&claude_skills, source))?
        {
            let entry = entry.map_err(|source| io_error(&claude_skills, source))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || is_legacy_ad_managed_symlink(&path) {
                continue;
            }
            if !path.join("SKILL.md").exists() && !path.is_symlink() {
                continue;
            }
            entries.push(SkillEntry {
                name,
                path: path.to_string_lossy().into_owned(),
                source: SkillEntrySource::External,
                source_id: None,
                version: None,
                description: parse_skill_description(&path),
                scope: SkillScope::Global,
            });
        }
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub(crate) fn is_legacy_ad_managed_symlink(entry: &Path) -> bool {
    if !entry.is_symlink() {
        return false;
    }
    let Ok(target) = std::fs::read_link(entry) else {
        return false;
    };
    if skill_library_dir()
        .ok()
        .as_deref()
        .is_some_and(|library| target_is_within(&target, library))
    {
        return true;
    }
    load_sources().unwrap_or_default().iter().any(|source| {
        let root = match source.source_type {
            SkillSourceType::Local => PathBuf::from(&source.url),
            SkillSourceType::Git => skill_library_dir()
                .map(|library| library.join(&source.id))
                .unwrap_or_default(),
        };
        target_is_within(&target, &root)
    })
}

fn load_sources() -> Result<Vec<SkillSource>, LegacySkillDiscoveryError> {
    let path =
        skill_sources_path().map_err(|error| LegacySkillDiscoveryError::Path(error.to_string()))?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(io_error(&path, source)),
    }
}

fn discover_unregistered_sources(
    sources: &mut Vec<SkillSource>,
) -> Result<(), LegacySkillDiscoveryError> {
    let library =
        skill_library_dir().map_err(|error| LegacySkillDiscoveryError::Path(error.to_string()))?;
    if !library.is_dir() {
        return Ok(());
    }
    let known = sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut discovered = Vec::new();
    for entry in std::fs::read_dir(&library).map_err(|source| io_error(&library, source))? {
        let entry = entry.map_err(|source| io_error(&library, source))?;
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || known.contains(name.as_str()) {
            continue;
        }
        let is_git = entry.path().join(".git").exists();
        discovered.push(SkillSource {
            id: name,
            source_type: if is_git {
                SkillSourceType::Git
            } else {
                SkillSourceType::Local
            },
            url: entry.path().to_string_lossy().into_owned(),
            branch: None,
            subdirectory: None,
            auto_update: false,
            added_at: Utc::now(),
        });
    }
    sources.extend(discovered);
    Ok(())
}

fn skills_root_for(source: &SkillSource) -> Option<PathBuf> {
    let base = match source.source_type {
        SkillSourceType::Git => skill_library_dir().ok()?.join(&source.id),
        SkillSourceType::Local => PathBuf::from(&source.url),
    };
    Some(match source.subdirectory.as_deref() {
        Some(subdirectory) => base.join(subdirectory),
        None => base,
    })
}

fn find_skill_dirs(directory: &Path, max_depth: usize, output: &mut Vec<PathBuf>) {
    if max_depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        if path.join("SKILL.md").is_file() {
            output.push(path);
        } else {
            find_skill_dirs(&path, max_depth - 1, output);
        }
    }
}

fn parse_skill_description(directory: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(directory.join("SKILL.md")).ok()?;
    let mut buffer = [0_u8; 2048];
    let length = file.read(&mut buffer).ok()?;
    let content = std::str::from_utf8(&buffer[..length]).ok()?;
    let frontmatter = content
        .trim_start()
        .strip_prefix("---")?
        .split_once("---")?
        .0;
    let lines = frontmatter.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let Some(value) = line.trim().strip_prefix("description:") else {
            continue;
        };
        let value = value.trim();
        if matches!(value, ">" | "|") {
            let continuation = lines[index + 1..]
                .iter()
                .take_while(|line| line.starts_with(' ') || line.starts_with('\t'))
                .map(|line| line.trim())
                .take_while(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            return (!continuation.is_empty()).then(|| truncate(&continuation, 200));
        }
        if !value.is_empty() {
            return Some(truncate(value.trim_matches(['"', '\'']), 200));
        }
    }
    None
}

fn truncate(value: &str, max_characters: usize) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(max_characters).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn git_version_for(source: &SkillSource) -> Option<String> {
    if source.source_type != SkillSourceType::Git {
        return None;
    }
    let repository = skill_library_dir().ok()?.join(&source.id);
    crate::fs::git::head_hash(&repository).ok()
}

fn determine_scope(name: &str, global_skills: &Path, project_path: Option<&str>) -> SkillScope {
    let global = global_skills.join(name);
    if (global.exists() || global.is_symlink()) && is_legacy_ad_managed_symlink(&global) {
        return SkillScope::Global;
    }
    if let Some(project_path) = project_path {
        let project = Path::new(project_path).join(".claude/skills").join(name);
        if (project.exists() || project.is_symlink()) && is_legacy_ad_managed_symlink(&project) {
            return SkillScope::Project;
        }
    }
    SkillScope::None
}

fn target_is_within(target: &Path, root: &Path) -> bool {
    if target.starts_with(root) {
        return true;
    }
    let Ok(canonical_root) = std::fs::canonicalize(root) else {
        return false;
    };
    target.starts_with(&canonical_root)
        || std::fs::canonicalize(target)
            .map(|canonical_target| canonical_target.starts_with(canonical_root))
            .unwrap_or(false)
}

fn io_error(path: &Path, source: std::io::Error) -> LegacySkillDiscoveryError {
    LegacySkillDiscoveryError::Io {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_path_check_accepts_a_canonical_dangling_target() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skill-library");
        std::fs::create_dir_all(&root).unwrap();
        let target = std::fs::canonicalize(&root).unwrap().join("missing-skill");

        assert!(target_is_within(&target, &root));
    }
}

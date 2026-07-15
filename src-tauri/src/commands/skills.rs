//! Skill source CRUD, library scanning, per-project toggle, and symlink management.
//!
//! Storage layout:
//!   ~/.ad/skill-library/<source_id>/   ← cloned repo or local dir reference
//!   ~/.ad/state/skill_sources.json     ← source configs
//!   ~/.ad/state/project_skills/<slug>.json ← per-project skill config
//!
//! The cloned repo IS the skill directory. No separate install/copy step.
//! Skills reference sibling dirs (bin/, scripts/) via relative paths,
//! so the repo structure must stay intact.
//!
//! Activation via symlinks:
//!   ~/.claude/skills/<name> → ~/.ad/skill-library/<id>/<subdir>/<name>/  (global)
//!   <project>/.claude/skills/<name> → same target                       (project)

use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;

use std::collections::BTreeMap;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::{
    claude_dir, claude_skills_dir, ensure_dir, project_skills_dir, skill_library_dir,
    skill_sources_path,
};
use crate::models::{
    ProjectSkillConfig, SkillEntry, SkillEntrySource,
    SkillListMode, SkillScope, SkillSource, SkillSourceType, SkillUpdateResult,
};

use super::{CmdResult, CommandError};

// ---------------------------------------------------------------------------
// Internal load/save helpers
// ---------------------------------------------------------------------------

fn load_sources() -> CmdResult<Vec<SkillSource>> {
    let path = skill_sources_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_sources(sources: &[SkillSource]) -> CmdResult<()> {
    let path = skill_sources_path()?;
    ensure_dir(path.parent().unwrap())?;
    let bytes = serde_json::to_vec_pretty(sources)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

fn project_skills_path(project_path: &str) -> CmdResult<PathBuf> {
    let slug = super::apply::slug_from_path(project_path);
    let dir = project_skills_dir()?;
    ensure_dir(&dir)?;
    Ok(dir.join(format!("{slug}.json")))
}

fn load_project_skills(project_path: &str) -> CmdResult<ProjectSkillConfig> {
    let path = project_skills_path(project_path)?;
    if !path.exists() {
        return Ok(ProjectSkillConfig {
            project_path: project_path.to_string(),
            listed_skills: Vec::new(),
            mode: SkillListMode::default(),
        });
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_project_skills(config: &ProjectSkillConfig) -> CmdResult<()> {
    let path = project_skills_path(&config.project_path)?;
    let bytes = serde_json::to_vec_pretty(config)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolve the skills root for a given source
// ---------------------------------------------------------------------------

fn skills_root_for(source: &SkillSource) -> CmdResult<PathBuf> {
    let base = match source.source_type {
        SkillSourceType::Git => skill_library_dir()?.join(&source.id),
        SkillSourceType::Local => PathBuf::from(&source.url),
    };
    match &source.subdirectory {
        Some(sub) => Ok(base.join(sub)),
        None => Ok(base),
    }
}

// ---------------------------------------------------------------------------
// SKILL.md frontmatter parsing
// ---------------------------------------------------------------------------

fn parse_skill_md(dir: &Path) -> Option<String> {
    use std::io::Read;
    let skill_md = dir.join("SKILL.md");
    let mut file = std::fs::File::open(&skill_md).ok()?;
    let mut buf = [0u8; 2048];
    let n = file.read(&mut buf).ok()?;
    let content = std::str::from_utf8(&buf[..n]).ok()?;
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];

    let lines: Vec<&str> = frontmatter.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed_line = line.trim();
        if let Some(rest) = trimmed_line.strip_prefix("description:") {
            let rest = rest.trim();
            if rest == ">" || rest == "|" {
                // YAML multi-line: collect indented continuation lines
                let mut parts = Vec::new();
                for cont in &lines[i + 1..] {
                    let cl = cont.trim();
                    if cl.is_empty() {
                        break;
                    }
                    if cont.starts_with(' ') || cont.starts_with('\t') {
                        parts.push(cl);
                    } else {
                        break;
                    }
                }
                if !parts.is_empty() {
                    let joined = parts.join(" ");
                    return Some(truncate_str(&joined, 200));
                }
            } else if !rest.is_empty() {
                let desc = rest.trim_matches('"').trim_matches('\'');
                return Some(truncate_str(desc, 200));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Symlink helpers
// ---------------------------------------------------------------------------

pub(crate) fn is_ad_managed_symlink(entry: &Path) -> bool {
    if !entry.is_symlink() {
        return false;
    }
    let Ok(target) = std::fs::read_link(entry) else {
        return false;
    };
    if let Ok(lib_dir) = skill_library_dir() {
        if target.starts_with(&lib_dir) {
            return true;
        }
    }
    if let Ok(sources) = load_sources() {
        for source in &sources {
            let source_path = match source.source_type {
                SkillSourceType::Local => PathBuf::from(&source.url),
                SkillSourceType::Git => skill_library_dir()
                    .map(|d| d.join(&source.id))
                    .unwrap_or_default(),
            };
            if target.starts_with(&source_path) {
                return true;
            }
        }
    }
    false
}

fn create_skill_symlink(link_dir: &Path, name: &str, target: &Path) -> CmdResult<()> {
    ensure_dir(link_dir)?;
    let link_path = link_dir.join(name);
    if link_path.exists() || link_path.is_symlink() {
        if is_ad_managed_symlink(&link_path) {
            std::fs::remove_file(&link_path)
                .with_context(|| format!("remove existing symlink {}", link_path.display()))?;
        } else {
            return Err(CommandError::Generic(format!(
                "entry already exists and is not AD-managed: {}",
                link_path.display()
            )));
        }
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link_path)
        .with_context(|| format!("symlink {} → {}", link_path.display(), target.display()))?;
    Ok(())
}

fn remove_skill_symlink(link_dir: &Path, name: &str) -> CmdResult<bool> {
    let link_path = link_dir.join(name);
    if !link_path.exists() && !link_path.is_symlink() {
        return Ok(false);
    }
    if !is_ad_managed_symlink(&link_path) {
        return Ok(false);
    }
    std::fs::remove_file(&link_path)
        .with_context(|| format!("remove symlink {}", link_path.display()))?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Tauri commands: Skill Source CRUD
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_skill_sources() -> CmdResult<Vec<SkillSource>> {
    let mut sources = load_sources()?;
    if auto_register_discovered(&mut sources)? {
        save_sources(&sources)?;
    }
    Ok(sources)
}

fn auto_register_discovered(sources: &mut Vec<SkillSource>) -> CmdResult<bool> {
    let lib_dir = skill_library_dir()?;
    if !lib_dir.is_dir() {
        return Ok(false);
    }
    let known: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
    let mut changed = false;
    if let Ok(rd) = std::fs::read_dir(&lib_dir) {
        for entry in rd {
            let Ok(entry) = entry else { continue };
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || known.contains(&name) {
                continue;
            }
            let is_git = entry.path().join(".git").exists();
            let url = if is_git {
                git_remote_url(&entry.path()).unwrap_or_else(|| entry.path().to_string_lossy().into_owned())
            } else {
                entry.path().to_string_lossy().into_owned()
            };
            sources.push(SkillSource {
                id: name,
                source_type: if is_git { SkillSourceType::Git } else { SkillSourceType::Local },
                url,
                branch: None,
                subdirectory: None,
                auto_update: false,
                added_at: Utc::now(),
            });
            changed = true;
        }
    }
    Ok(changed)
}

fn git_remote_url(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

#[tauri::command]
pub fn add_skill_source(source: SkillSource) -> CmdResult<SkillSource> {
    if source.id.is_empty() {
        return Err(CommandError::Generic("source id is empty".into()));
    }
    let mut sources = load_sources()?;
    if sources.iter().any(|s| s.id == source.id) {
        return Err(CommandError::Generic(format!(
            "source id already exists: {}",
            source.id
        )));
    }

    match source.source_type {
        SkillSourceType::Git => {
            let dest = skill_library_dir()?.join(&source.id);
            ensure_dir(dest.parent().unwrap())?;
            crate::fs::git::clone(&source.url, &dest, source.branch.as_deref())?;
        }
        SkillSourceType::Local => {
            let local_path = Path::new(&source.url);
            if !local_path.is_dir() {
                return Err(CommandError::Generic(format!(
                    "local path is not a directory: {}",
                    source.url
                )));
            }
        }
    }

    let mut saved = source;
    saved.added_at = Utc::now();
    sources.push(saved.clone());
    save_sources(&sources)?;
    Ok(saved)
}

#[tauri::command]
pub fn remove_skill_source(id: String) -> CmdResult<()> {
    let mut sources = load_sources()?;
    let before = sources.len();
    sources.retain(|s| s.id != id);
    if sources.len() == before {
        return Err(CommandError::Generic(format!("source not found: {id}")));
    }
    save_sources(&sources)?;

    let lib_dir = skill_library_dir()?.join(&id);
    if lib_dir.exists() {
        std::fs::remove_dir_all(&lib_dir)
            .with_context(|| format!("remove {}", lib_dir.display()))?;
    }

    clean_dangling_symlinks()?;
    Ok(())
}

#[tauri::command]
pub fn update_skill_source(id: String) -> CmdResult<SkillUpdateResult> {
    let sources = load_sources()?;
    let source = sources
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| CommandError::Generic(format!("source not found: {id}")))?;

    match source.source_type {
        SkillSourceType::Git => {
            let repo_dir = skill_library_dir()?.join(&id);
            if !repo_dir.exists() {
                return Err(CommandError::Generic(format!(
                    "repo not cloned yet for source: {id}"
                )));
            }
            let result = crate::fs::git::pull(&repo_dir)?;
            Ok(SkillUpdateResult {
                source_id: id,
                updated: result.updated,
                before_version: result.before,
                after_version: result.after,
            })
        }
        SkillSourceType::Local => Ok(SkillUpdateResult {
            source_id: id,
            updated: false,
            before_version: "local".into(),
            after_version: "local".into(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands: Skill Library scan
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn scan_skill_library(project_path: Option<String>) -> CmdResult<Vec<SkillEntry>> {
    scan_skill_library_inner(project_path, true)
}

pub(crate) fn scan_skill_library_read_only(
    project_path: Option<String>,
) -> CmdResult<Vec<SkillEntry>> {
    scan_skill_library_inner(project_path, false)
}

fn scan_skill_library_inner(
    project_path: Option<String>,
    persist_discovered_sources: bool,
) -> CmdResult<Vec<SkillEntry>> {
    let sources = load_sources()?;
    let claude_skills = claude_skills_dir()?;
    let mut entries = Vec::new();

    let mut sources = sources;
    if auto_register_discovered(&mut sources)? && persist_discovered_sources {
        save_sources(&sources)?;
    }

    for source in &sources {
        let root = match skills_root_for(source) {
            Ok(r) => r,
            Err(_) => continue,
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
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let scope = determine_scope(&name, &claude_skills, project_path.as_deref());

            entries.push(SkillEntry {
                name: name.clone(),
                path: skill_path.to_string_lossy().into_owned(),
                source: SkillEntrySource::Managed,
                source_id: Some(source.id.clone()),
                version: version.clone(),
                description: parse_skill_md(&skill_path),
                scope,
            });
        }
    }

    // Scan ~/.claude/skills/ for external (non-AD-managed) skills
    if claude_skills.exists() {
        if let Ok(rd) = std::fs::read_dir(&claude_skills) {
            for entry in rd {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if is_ad_managed_symlink(&path) {
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
                    description: parse_skill_md(&path),
                    scope: SkillScope::Global,
                });
            }
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn find_skill_dirs(dir: &Path, max_depth: usize, out: &mut Vec<PathBuf>) {
    if max_depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.join("SKILL.md").exists() {
            out.push(path);
        } else {
            find_skill_dirs(&path, max_depth - 1, out);
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn git_version_for(source: &SkillSource) -> Option<String> {
    if source.source_type != SkillSourceType::Git {
        return None;
    }
    let repo_dir = skill_library_dir().ok()?.join(&source.id);
    crate::fs::git::head_hash(&repo_dir).ok()
}

fn determine_scope(name: &str, claude_skills: &Path, project_path: Option<&str>) -> SkillScope {
    let global_link = claude_skills.join(name);
    if (global_link.exists() || global_link.is_symlink()) && is_ad_managed_symlink(&global_link) {
        return SkillScope::Global;
    }
    if let Some(pp) = project_path {
        let project_link = Path::new(pp).join(".claude/skills").join(name);
        if (project_link.exists() || project_link.is_symlink())
            && is_ad_managed_symlink(&project_link)
        {
            return SkillScope::Project;
        }
    }
    SkillScope::None
}

// ---------------------------------------------------------------------------
// Tauri commands: Per-project skill toggle + scope
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_project_skills(project_path: String) -> CmdResult<ProjectSkillConfig> {
    load_project_skills(&project_path)
}

#[tauri::command]
pub fn toggle_skill(
    project_path: String,
    skill_id: String,
    enabled: bool,
) -> CmdResult<ProjectSkillConfig> {
    let mut config = load_project_skills(&project_path)?;

    match config.mode {
        SkillListMode::Blocklist => {
            if enabled {
                config.listed_skills.retain(|s| s != &skill_id);
            } else if !config.listed_skills.contains(&skill_id) {
                config.listed_skills.push(skill_id.clone());
            }
        }
        SkillListMode::Allowlist => {
            if enabled && !config.listed_skills.contains(&skill_id) {
                config.listed_skills.push(skill_id.clone());
            } else if !enabled {
                config.listed_skills.retain(|s| s != &skill_id);
            }
        }
    }

    save_project_skills(&config)?;

    let (source_id, name) = split_skill_id(&skill_id)?;
    let skill_dir = resolve_skill_dir(source_id, name)?;
    let project_skills_dir = Path::new(&project_path).join(".claude/skills");

    if enabled {
        if let Some(dir) = skill_dir {
            create_skill_symlink(&project_skills_dir, name, &dir)?;
        }
    } else {
        remove_skill_symlink(&project_skills_dir, name)?;
    }

    Ok(config)
}

fn resolve_skill_dir(source_id: &str, name: &str) -> CmdResult<Option<PathBuf>> {
    let sources = load_sources()?;
    let source = match sources.iter().find(|s| s.id == source_id) {
        Some(s) => s,
        None => return Ok(None),
    };
    let root = skills_root_for(source)?;
    let mut all_skills = Vec::new();
    find_skill_dirs(&root, 4, &mut all_skills);
    Ok(all_skills.into_iter().find(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy() == name)
            .unwrap_or(false)
    }))
}

#[tauri::command]
pub fn set_skill_scope(skill_id: String, scope: String) -> CmdResult<()> {
    let (source_id, name) = split_skill_id(&skill_id)?;
    let skill_dir = resolve_skill_dir(source_id, name)?
        .ok_or_else(|| CommandError::Generic(format!("skill not found: {skill_id}")))?;

    let claude_skills = claude_skills_dir()?;

    match scope.as_str() {
        "global" => {
            create_skill_symlink(&claude_skills, name, &skill_dir)?;
        }
        "none" => {
            remove_skill_symlink(&claude_skills, name)?;
        }
        other => {
            return Err(CommandError::Generic(format!(
                "unknown scope: {other} (expected global or none)"
            )));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn apply_project_skills(project_path: String) -> CmdResult<Vec<String>> {
    let config = load_project_skills(&project_path)?;
    let sources = load_sources()?;
    let project_skills_dir = Path::new(&project_path).join(".claude/skills");
    let mut applied = Vec::new();

    // Remove all AD-managed symlinks first
    if project_skills_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&project_skills_dir) {
            for entry in rd {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if is_ad_managed_symlink(&path) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    // Collect all available skill IDs from all sources
    let mut all_skills: Vec<(String, PathBuf)> = Vec::new();
    for source in &sources {
        let root = match skills_root_for(source) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !root.is_dir() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&root) {
            for entry in rd {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if !path.is_dir() || !path.join("SKILL.md").exists() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let skill_id = format!("{}/{}", source.id, name);
                all_skills.push((skill_id, path));
            }
        }
    }

    // Apply based on mode
    let active_ids: Vec<String> = match config.mode {
        SkillListMode::Blocklist => all_skills
            .iter()
            .map(|(id, _)| id.clone())
            .filter(|id| !config.listed_skills.contains(id))
            .collect(),
        SkillListMode::Allowlist => config.listed_skills.clone(),
    };

    let claude_skills = claude_skills_dir()?;
    for skill_id in &active_ids {
        let Some((_, skill_dir)) = all_skills.iter().find(|(id, _)| id == skill_id) else {
            continue;
        };
        let name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // Skip if already global
        let global_link = claude_skills.join(&name);
        if global_link.is_symlink() && is_ad_managed_symlink(&global_link) {
            continue;
        }

        if let Ok(()) = create_skill_symlink(&project_skills_dir, &name, skill_dir) {
            applied.push(skill_id.clone());
        }
    }

    Ok(applied)
}

fn split_skill_id(skill_id: &str) -> CmdResult<(&str, &str)> {
    skill_id.split_once('/').ok_or_else(|| {
        CommandError::Generic(format!(
            "invalid skill_id format: {skill_id} (expected source_id/name)"
        ))
    })
}

fn clean_dangling_symlinks() -> CmdResult<()> {
    let claude_skills = claude_skills_dir()?;
    if claude_skills.exists() {
        if let Ok(rd) = std::fs::read_dir(&claude_skills) {
            for entry in rd {
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if path.is_symlink() && is_ad_managed_symlink(&path) {
                    let target = std::fs::read_link(&path).ok();
                    if target.as_ref().map_or(true, |t| !t.exists()) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin management — toggle entire plugins per project via settings.local.json
// ---------------------------------------------------------------------------

/// A plugin entry read from ~/.claude/settings.json's enabledPlugins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub enabled: bool,
}

#[tauri::command]
pub fn list_plugins(project_path: Option<String>) -> CmdResult<Vec<PluginInfo>> {
    let global = read_global_enabled_plugins()?;
    let project_overrides = match &project_path {
        Some(pp) => read_project_plugin_overrides(pp)?,
        None => BTreeMap::new(),
    };

    let mut result: Vec<PluginInfo> = global
        .into_iter()
        .map(|(id, enabled)| {
            let effective = project_overrides.get(&id).copied().unwrap_or(enabled);
            PluginInfo { id, enabled: effective }
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

#[tauri::command]
pub fn toggle_plugin(project_path: String, plugin_id: String, enabled: bool) -> CmdResult<()> {
    let local_path = Path::new(&project_path).join(".claude/settings.local.json");
    let mut local: serde_json::Value = if local_path.exists() {
        let bytes = std::fs::read(&local_path)
            .with_context(|| format!("read {}", local_path.display()))?;
        serde_json::from_slice(&bytes)?
    } else {
        serde_json::json!({})
    };

    let plugins = local
        .as_object_mut()
        .ok_or_else(|| CommandError::Generic("settings.local.json is not an object".into()))?
        .entry("enabledPlugins")
        .or_insert_with(|| serde_json::json!({}));

    plugins
        .as_object_mut()
        .ok_or_else(|| CommandError::Generic("enabledPlugins is not an object".into()))?
        .insert(plugin_id, serde_json::Value::Bool(enabled));

    ensure_dir(local_path.parent().unwrap())?;
    let bytes = serde_json::to_vec_pretty(&local)?;
    write_atomic(&local_path, &bytes)?;
    Ok(())
}

fn read_global_enabled_plugins() -> CmdResult<BTreeMap<String, bool>> {
    let path = claude_dir()?.join("settings.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let val: serde_json::Value = serde_json::from_slice(&bytes)?;
    let Some(obj) = val.get("enabledPlugins").and_then(|v| v.as_object()) else {
        return Ok(BTreeMap::new());
    };
    let mut map = BTreeMap::new();
    for (k, v) in obj {
        map.insert(k.clone(), v.as_bool().unwrap_or(false));
    }
    Ok(map)
}

fn read_project_plugin_overrides(project_path: &str) -> CmdResult<BTreeMap<String, bool>> {
    let path = Path::new(project_path).join(".claude/settings.local.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let val: serde_json::Value = serde_json::from_slice(&bytes)?;
    let Some(obj) = val.get("enabledPlugins").and_then(|v| v.as_object()) else {
        return Ok(BTreeMap::new());
    };
    let mut map = BTreeMap::new();
    for (k, v) in obj {
        map.insert(k.clone(), v.as_bool().unwrap_or(false));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());
        tmp
    }

    fn make_skill_in_source(base: &Path, source: &str, name: &str) -> PathBuf {
        let dir = base.join(".ad/skill-library").join(source).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill {name}\n---\nContent"),
        )
        .unwrap();
        dir
    }

    fn register_source(id: &str, base: &Path) {
        let source = SkillSource {
            id: id.into(),
            source_type: SkillSourceType::Local,
            url: base
                .join(".ad/skill-library")
                .join(id)
                .to_string_lossy()
                .into_owned(),
            branch: None,
            subdirectory: None,
            auto_update: false,
            added_at: Utc::now(),
        };
        let mut sources = load_sources().unwrap();
        sources.push(source);
        save_sources(&sources).unwrap();
    }

    #[test]
    #[serial(home_env)]
    fn load_save_sources_roundtrip() {
        let _t = setup();
        let s = SkillSource {
            id: "test".into(),
            source_type: SkillSourceType::Local,
            url: "/tmp/skills".into(),
            branch: None,
            subdirectory: None,
            auto_update: false,
            added_at: Utc::now(),
        };
        save_sources(&[s.clone()]).unwrap();
        let loaded = load_sources().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test");
    }

    #[test]
    #[serial(home_env)]
    fn scan_empty_library() {
        let _t = setup();
        let entries = scan_skill_library(None).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn scan_finds_skills_from_source() {
        let t = setup();
        make_skill_in_source(t.path(), "my-source", "skill-a");
        make_skill_in_source(t.path(), "my-source", "skill-b");
        register_source("my-source", t.path());

        let entries = scan_skill_library(None).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.name == "skill-a"));
        assert!(entries.iter().any(|e| e.name == "skill-b"));
        assert_eq!(entries[0].source, SkillEntrySource::Managed);
    }

    #[test]
    #[serial(home_env)]
    fn parse_skill_md_extracts_description() {
        let t = setup();
        let dir = make_skill_in_source(t.path(), "src", "foo");
        let desc = parse_skill_md(&dir);
        assert_eq!(desc.as_deref(), Some("Test skill foo"));
    }

    #[test]
    #[serial(home_env)]
    fn toggle_creates_and_removes_symlink() {
        let t = setup();
        make_skill_in_source(t.path(), "src", "qa");
        register_source("src", t.path());

        let project_dir = t.path().join("my-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let project_path = project_dir.to_string_lossy().into_owned();

        toggle_skill(project_path.clone(), "src/qa".into(), true).unwrap();
        let link = project_dir.join(".claude/skills/qa");
        assert!(link.is_symlink(), "symlink should exist after toggle on");

        let target = std::fs::read_link(&link).unwrap();
        assert!(
            target.starts_with(skill_library_dir().unwrap()),
            "symlink target should point into skill-library"
        );

        toggle_skill(project_path, "src/qa".into(), false).unwrap();
        assert!(!link.exists(), "symlink should be removed after toggle off");
    }

    #[test]
    #[serial(home_env)]
    fn set_scope_global_creates_global_symlink() {
        let t = setup();
        make_skill_in_source(t.path(), "src", "review");
        register_source("src", t.path());

        let claude_dir = t.path().join(".claude/skills");
        std::fs::create_dir_all(&claude_dir).unwrap();

        set_skill_scope("src/review".into(), "global".into()).unwrap();
        let link = claude_dir.join("review");
        assert!(link.is_symlink());
        assert!(is_ad_managed_symlink(&link));

        set_skill_scope("src/review".into(), "none".into()).unwrap();
        assert!(!link.exists());
    }

    #[test]
    #[serial(home_env)]
    fn is_ad_managed_distinguishes_external() {
        let t = setup();
        let claude_dir = t.path().join(".claude/skills");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let external_target = t.path().join("external-skill");
        std::fs::create_dir_all(&external_target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external_target, claude_dir.join("ext")).unwrap();
        assert!(!is_ad_managed_symlink(&claude_dir.join("ext")));

        let managed_target = make_skill_in_source(t.path(), "src", "managed");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&managed_target, claude_dir.join("managed")).unwrap();
        assert!(is_ad_managed_symlink(&claude_dir.join("managed")));
    }

    #[test]
    #[serial(home_env)]
    fn apply_reconciles_symlinks_from_config() {
        let t = setup();
        make_skill_in_source(t.path(), "src", "qa");
        make_skill_in_source(t.path(), "src", "review");
        register_source("src", t.path());

        let project_dir = t.path().join("proj");
        std::fs::create_dir(&project_dir).unwrap();
        let project_path = project_dir.to_string_lossy().into_owned();

        // Blocklist: disable "review", keep "qa" enabled
        let config = ProjectSkillConfig {
            project_path: project_path.clone(),
            listed_skills: vec!["src/review".into()],
            mode: SkillListMode::Blocklist,
        };
        save_project_skills(&config).unwrap();

        let applied = apply_project_skills(project_path).unwrap();
        assert_eq!(applied, vec!["src/qa".to_string()]);

        let qa_link = project_dir.join(".claude/skills/qa");
        let review_link = project_dir.join(".claude/skills/review");
        assert!(qa_link.is_symlink(), "qa should be enabled");
        assert!(!review_link.exists(), "review should be disabled");
    }

    #[test]
    fn split_skill_id_parses() {
        let (src, name) = split_skill_id("gstack/qa").unwrap();
        assert_eq!(src, "gstack");
        assert_eq!(name, "qa");
        assert!(split_skill_id("no-slash").is_err());
    }
}

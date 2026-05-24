//! Layer-aware profile application (D8 + D10).
//!
//! Applies a subset of a profile's `layers` to a target project:
//! - `shared` layer → `<project>/.claude/settings.json` (committed to git)
//! - `local` layer → `<project>/.claude/settings.local.json` (gitignored)
//! - `env` layer → returned as `export` snippet for the user to paste
//!
//! For shared/local layers, the existing file (if any) is deep-merged with
//! `incoming` via `fs::merge::merge`. Conflicts cause the call to return
//! `ApplyOutcome::NeedsResolution`; the frontend collects user choices and
//! re-invokes with `resolutions` populated.
//!
//! Backups are written to `~/.ad/backups/<ISO8601>-<slug>-<layer>.json`
//! before any overwrite. The path slug is derived from the project path
//! (D10) so all backups for the same project sort together by timestamp.
//!
//! On success the project's `last_applied` field in `~/.ad/state/projects.json`
//! is refreshed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fs::atomic::write_atomic;
use crate::fs::merge::{merge, Conflict, MergeOutcome, Resolution};
use crate::fs::paths::{backups_dir, ensure_dir};
use crate::models::LastApplied;

use super::profiles;
use super::projects;
use super::{CmdResult, CommandError};

/// Caller-supplied options for `apply_profile_to_project`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOptions {
    /// Subset of `["shared", "local", "env"]`. Empty = nothing to do.
    pub layers: Vec<String>,
    /// Map from `Conflict.key_path` to `Resolution`. Use the `layer` prefix
    /// when the same key appears in multiple layers; the apply call only
    /// looks at resolutions whose key equals the conflict's key (the layer
    /// is implicit because each layer is merged independently).
    #[serde(default)]
    pub resolutions: HashMap<String, Resolution>,
    /// User has acknowledged the "git dirty" warning. Required when applying
    /// the `shared` layer to a dirty git repo.
    #[serde(default)]
    pub overwrite_dirty_warning_acked: bool,
}

/// Result of a successful apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub profile_id: String,
    pub project_path: String,
    pub written_files: Vec<String>,
    pub backup_paths: Vec<String>,
    /// Present when `env` layer was requested. Multi-line `export ...` block.
    pub env_export_snippet: Option<String>,
    pub warnings: Vec<String>,
    pub conflicts_resolved: usize,
}

/// Outcome of an apply attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ApplyOutcome {
    Applied(ApplyResult),
    /// Conflicts were found in `layer` that aren't covered by `options.resolutions`.
    NeedsResolution {
        layer: String,
        conflicts: Vec<Conflict>,
    },
    /// `shared` layer requested but the project's git tree is dirty and the
    /// user hasn't ack'd. Caller should show a warning, get ack, then retry.
    GitDirtyBlocked {
        message: String,
    },
}

#[tauri::command]
pub fn apply_profile_to_project(
    profile_id: String,
    project_path: String,
    options: ApplyOptions,
) -> CmdResult<ApplyOutcome> {
    let profile = profiles::get_profile(profile_id.clone())?;
    let projects_list = projects::load()?;
    let project = projects_list
        .iter()
        .find(|p| p.path == project_path)
        .cloned()
        .ok_or_else(|| {
            CommandError::Generic(format!(
                "project not registered: {project_path}; add it first via add_project"
            ))
        })?;

    let project_dir = PathBuf::from(&project.path);
    if !project_dir.is_dir() {
        return Err(CommandError::Generic(format!(
            "project path is not a directory: {}",
            project.path
        )));
    }

    // Pre-flight: git dirty check for shared layer.
    if options.layers.iter().any(|l| l == "shared")
        && !options.overwrite_dirty_warning_acked
        && super::projects::get_project_status(project.path.clone())?.git_dirty
    {
        return Ok(ApplyOutcome::GitDirtyBlocked {
            message: "Working tree has uncommitted changes. Set overwriteDirtyWarningAcked=true to apply anyway.".into(),
        });
    }

    let mut written_files = Vec::new();
    let mut backup_paths = Vec::new();
    let mut warnings = Vec::new();
    let mut env_export_snippet: Option<String> = None;
    let mut conflicts_resolved = 0usize;
    let now = Utc::now();

    let claude_dir = project_dir.join(".claude");

    for layer in &options.layers {
        match layer.as_str() {
            "shared" => {
                let Some(incoming) = profile.layers.shared.as_ref() else {
                    warnings.push("profile has no `shared` layer; nothing to apply".into());
                    continue;
                };
                let target = claude_dir.join("settings.json");
                let outcome = write_layer(
                    &target,
                    incoming,
                    None,
                    &options.resolutions,
                    &project.path,
                    "shared",
                    now,
                )?;
                match outcome {
                    LayerWriteOutcome::Wrote { backup, n_resolved } => {
                        written_files.push(target.to_string_lossy().into_owned());
                        if let Some(b) = backup {
                            backup_paths.push(b);
                        }
                        conflicts_resolved += n_resolved;
                    }
                    LayerWriteOutcome::Conflicts(cs) => {
                        return Ok(ApplyOutcome::NeedsResolution {
                            layer: "shared".into(),
                            conflicts: cs,
                        });
                    }
                }
            }
            "local" => {
                // Profile template content lives in the shared layer; apply it
                // to settings.local.json so the project has its own editable copy.
                let incoming = profile.layers.shared.as_ref()
                    .or(profile.layers.local.as_ref());
                let Some(incoming) = incoming else {
                    warnings.push("profile has no settings; nothing to apply".into());
                    continue;
                };
                let target = claude_dir.join("settings.local.json");
                let outcome = write_layer(
                    &target,
                    incoming,
                    None,
                    &options.resolutions,
                    &project.path,
                    "local",
                    now,
                )?;
                match outcome {
                    LayerWriteOutcome::Wrote { backup, n_resolved } => {
                        written_files.push(target.to_string_lossy().into_owned());
                        if let Some(b) = backup {
                            backup_paths.push(b);
                        }
                        conflicts_resolved += n_resolved;
                    }
                    LayerWriteOutcome::Conflicts(cs) => {
                        return Ok(ApplyOutcome::NeedsResolution {
                            layer: "local".into(),
                            conflicts: cs,
                        });
                    }
                }
                // Check .gitignore for settings.local.json exclusion.
                let status = super::projects::get_project_status(project.path.clone())?;
                if status.is_git_repo && status.gitignore_excludes_settings_local == Some(false) {
                    warnings.push(
                        "settings.local.json is not in .gitignore; consider adding it so it doesn't get committed".into(),
                    );
                }
            }
            "env" => {
                if profile.layers.env.is_empty() {
                    warnings.push("profile has no `env` layer; nothing to copy".into());
                    continue;
                }
                let mut snippet = String::new();
                for (k, v) in &profile.layers.env {
                    // Shell-safe single-quote escaping.
                    let escaped = v.replace('\'', "'\\''");
                    snippet.push_str(&format!("export {k}='{escaped}'\n"));
                }
                env_export_snippet = Some(snippet);
            }
            other => {
                return Err(CommandError::Generic(format!(
                    "unknown layer: {other:?} (expected shared/local/env)"
                )));
            }
        }
    }

    // Update project state with last_applied.
    update_last_applied(
        &project.path,
        &profile_id,
        &options.layers,
        &backup_paths,
        conflicts_resolved,
        now,
    )?;

    Ok(ApplyOutcome::Applied(ApplyResult {
        profile_id,
        project_path: project.path,
        written_files,
        backup_paths,
        env_export_snippet,
        warnings,
        conflicts_resolved,
    }))
}

enum LayerWriteOutcome {
    Wrote {
        backup: Option<String>,
        n_resolved: usize,
    },
    Conflicts(Vec<Conflict>),
}

// `base_override`: reserved for future use — `None` merges against the existing target file.
// instead of the existing target file. `None` = old behaviour (merge against target).
fn write_layer(
    target: &Path,
    incoming: &Value,
    base_override: Option<&Value>,
    resolutions: &HashMap<String, Resolution>,
    project_path: &str,
    layer: &str,
    ts: chrono::DateTime<chrono::Utc>,
) -> CmdResult<LayerWriteOutcome> {
    // Determine the base to merge `incoming` against.
    let existing: Value = if let Some(base) = base_override {
        base.clone()
    } else if target.exists() {
        let bytes = std::fs::read(target).with_context(|| format!("read {}", target.display()))?;
        if bytes.is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", target.display()))?
        }
    } else {
        Value::Object(serde_json::Map::new())
    };

    let merged = match merge(&existing, incoming, resolutions) {
        MergeOutcome::Merged(v) => v,
        MergeOutcome::NeedsResolution(cs) => {
            return Ok(LayerWriteOutcome::Conflicts(cs));
        }
    };

    // Backup existing if present and non-trivial.
    let backup = if target.exists() {
        let bytes = std::fs::read(target)?;
        if !bytes.is_empty() {
            let backup_path = backup_path_for(project_path, layer, ts)?;
            ensure_dir(backup_path.parent().unwrap())?;
            write_atomic(&backup_path, &bytes)?;
            Some(backup_path.to_string_lossy().into_owned())
        } else {
            None
        }
    } else {
        None
    };

    ensure_dir(target.parent().unwrap())?;
    let pretty = serde_json::to_vec_pretty(&merged)?;
    write_atomic(target, &pretty)?;

    // Count how many resolutions actually applied to this layer's conflicts.
    // We don't know post-merge, but we can re-probe: re-run with empty
    // resolutions and count NeedsResolution count = conflicts that would
    // have been raised. The number of those actually keyed in `resolutions`
    // is what we report.
    let n_resolved = match merge(&existing, incoming, &HashMap::new()) {
        MergeOutcome::NeedsResolution(cs) => cs
            .iter()
            .filter(|c| resolutions.contains_key(&c.key_path))
            .count(),
        _ => 0,
    };

    Ok(LayerWriteOutcome::Wrote { backup, n_resolved })
}

fn backup_path_for(
    project_path: &str,
    layer: &str,
    ts: chrono::DateTime<chrono::Utc>,
) -> CmdResult<PathBuf> {
    let slug = slug_from_path(project_path);
    let stamp = ts.format("%Y-%m-%dT%H-%M-%SZ");
    Ok(backups_dir()?.join(format!("{stamp}-{slug}-{layer}.json")))
}

/// Path slug used in backup filenames. `/Users/x/projects/foo` → `users-x-projects-foo`.
/// Lowercases ASCII, replaces `/` and `_` with `-`, collapses repeated `-`.
pub(crate) fn slug_from_path(path: &str) -> String {
    let lower = path.trim_start_matches('/').to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut last_dash = false;
    for c in lower.chars() {
        let mapped = if c == '/' || c == '_' || c == ' ' {
            '-'
        } else {
            c
        };
        if mapped == '-' {
            if !last_dash {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    // Trim trailing dash.
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn update_last_applied(
    project_path: &str,
    profile_id: &str,
    layers: &[String],
    backup_paths: &[String],
    conflicts_resolved: usize,
    ts: chrono::DateTime<chrono::Utc>,
) -> CmdResult<()> {
    let mut all = projects::load()?;
    let p = all
        .iter_mut()
        .find(|p| p.path == project_path)
        .ok_or_else(|| CommandError::Generic("project disappeared mid-apply".into()))?;
    p.current_profile_id = Some(profile_id.to_string());
    p.last_applied = Some(LastApplied {
        profile_id: profile_id.to_string(),
        timestamp: ts,
        layers: layers.to_vec(),
        backup_paths: backup_paths.to_vec(),
        conflicts_resolved,
    });
    projects::save(&all)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::profiles::save_profile;
    use crate::commands::projects::add_project;
    use crate::models::{ProfileFile, ProfileLayers};
    use serial_test::serial;
    use tempfile::TempDir;

    fn setup_home() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());
        tmp
    }

    fn make_project(home: &Path, name: &str) -> PathBuf {
        let p = home.join(name);
        std::fs::create_dir(&p).unwrap();
        p
    }

    fn make_layered_profile(id: &str, layers: ProfileLayers) -> ProfileFile {
        let mut p = ProfileFile::sample();
        p.id = id.to_string();
        p.display_name = id.to_string();
        p.layers = layers;
        p.settings = Default::default();
        p
    }

    #[test]
    fn slug_replaces_slashes_and_underscores() {
        assert_eq!(
            slug_from_path("/Users/yuanxuan/ai_workspace/ad"),
            "users-yuanxuan-ai-workspace-ad"
        );
        assert_eq!(slug_from_path("/foo/bar"), "foo-bar");
        assert_eq!(slug_from_path("/x_y_z/_a"), "x-y-z-a");
    }

    #[test]
    #[serial(home_env)]
    fn apply_local_layer_writes_settings_local_json() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();

        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let profile = make_layered_profile(
            "work",
            ProfileLayers {
                shared: None,
                local: Some(serde_json::json!({"model": "claude-opus-4-7"})),
                env: Default::default(),
            },
        );
        save_profile(profile).unwrap();

        let outcome = apply_profile_to_project(
            "work".into(),
            proj_canonical.clone(),
            ApplyOptions {
                layers: vec!["local".into()],
                ..Default::default()
            },
        )
        .unwrap();

        match outcome {
            ApplyOutcome::Applied(r) => {
                assert_eq!(r.written_files.len(), 1);
                assert!(r.written_files[0].ends_with(".claude/settings.local.json"));
                let written: Value =
                    serde_json::from_slice(&std::fs::read(&r.written_files[0]).unwrap()).unwrap();
                assert_eq!(written["model"], "claude-opus-4-7");
                assert!(r.backup_paths.is_empty(), "no existing file → no backup");
            }
            other => panic!("expected Applied, got {other:?}"),
        }

        // Project state updated.
        let projs = projects::load().unwrap();
        assert_eq!(projs[0].current_profile_id.as_deref(), Some("work"));
        let la = projs[0].last_applied.as_ref().unwrap();
        assert_eq!(la.profile_id, "work");
        assert_eq!(la.layers, vec!["local".to_string()]);
    }

    #[test]
    #[serial(home_env)]
    fn apply_with_existing_file_merges_and_backs_up() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let claude_dir = proj.join(".claude");
        std::fs::create_dir(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.local.json"),
            r#"{"existing": true}"#,
        )
        .unwrap();

        let profile = make_layered_profile(
            "work",
            ProfileLayers {
                shared: None,
                local: Some(serde_json::json!({"model": "x"})),
                env: Default::default(),
            },
        );
        save_profile(profile).unwrap();

        let outcome = apply_profile_to_project(
            "work".into(),
            proj_canonical,
            ApplyOptions {
                layers: vec!["local".into()],
                ..Default::default()
            },
        )
        .unwrap();

        match outcome {
            ApplyOutcome::Applied(r) => {
                assert_eq!(r.backup_paths.len(), 1);
                let merged: Value = serde_json::from_slice(
                    &std::fs::read(claude_dir.join("settings.local.json")).unwrap(),
                )
                .unwrap();
                // Existing key preserved, profile key added.
                assert_eq!(merged["existing"], true);
                assert_eq!(merged["model"], "x");
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    #[test]
    #[serial(home_env)]
    fn apply_returns_needs_resolution_when_conflict() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let claude_dir = proj.join(".claude");
        std::fs::create_dir(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.local.json"),
            r#"{"model": "old"}"#,
        )
        .unwrap();

        let profile = make_layered_profile(
            "work",
            ProfileLayers {
                shared: None,
                local: Some(serde_json::json!({"model": "new"})),
                env: Default::default(),
            },
        );
        save_profile(profile).unwrap();

        let outcome = apply_profile_to_project(
            "work".into(),
            proj_canonical.clone(),
            ApplyOptions {
                layers: vec!["local".into()],
                ..Default::default()
            },
        )
        .unwrap();

        match outcome {
            ApplyOutcome::NeedsResolution { layer, conflicts } => {
                assert_eq!(layer, "local");
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].key_path, "model");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }

        // Re-invoke with a resolution: should succeed.
        let mut resolutions = HashMap::new();
        resolutions.insert("model".into(), Resolution::UseIncoming);
        let outcome2 = apply_profile_to_project(
            "work".into(),
            proj_canonical,
            ApplyOptions {
                layers: vec!["local".into()],
                resolutions,
                ..Default::default()
            },
        )
        .unwrap();
        let r = match outcome2 {
            ApplyOutcome::Applied(r) => r,
            other => panic!("expected Applied, got {other:?}"),
        };
        assert_eq!(r.conflicts_resolved, 1);
        let merged: Value =
            serde_json::from_slice(&std::fs::read(claude_dir.join("settings.local.json")).unwrap())
                .unwrap();
        assert_eq!(merged["model"], "new");
    }

    #[test]
    #[serial(home_env)]
    fn apply_env_layer_returns_export_snippet() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let mut env = std::collections::BTreeMap::new();
        env.insert("ANTHROPIC_API_KEY".into(), "sk-test".into());
        env.insert("ANTHROPIC_MODEL".into(), "claude-opus-4-7".into());
        let profile = make_layered_profile(
            "work",
            ProfileLayers {
                shared: None,
                local: None,
                env,
            },
        );
        save_profile(profile).unwrap();

        let outcome = apply_profile_to_project(
            "work".into(),
            proj_canonical,
            ApplyOptions {
                layers: vec!["env".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let r = match outcome {
            ApplyOutcome::Applied(r) => r,
            other => panic!("expected Applied, got {other:?}"),
        };
        let snippet = r.env_export_snippet.expect("env snippet present");
        assert!(snippet.contains("export ANTHROPIC_API_KEY='sk-test'"));
        assert!(snippet.contains("export ANTHROPIC_MODEL='claude-opus-4-7'"));
        // No files should have been written for env-only apply.
        assert!(r.written_files.is_empty());
    }

    #[test]
    #[serial(home_env)]
    fn apply_global_settings_json_is_never_written() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let profile = make_layered_profile(
            "work",
            ProfileLayers {
                shared: Some(serde_json::json!({"x": 1})),
                local: Some(serde_json::json!({"y": 2})),
                env: Default::default(),
            },
        );
        save_profile(profile).unwrap();

        let global = g.path().join(".claude/settings.json");
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        std::fs::write(&global, b"{\"untouched\":true}").unwrap();
        let before = std::fs::metadata(&global).unwrap().modified().unwrap();

        let _ = apply_profile_to_project(
            "work".into(),
            proj_canonical,
            ApplyOptions {
                layers: vec!["shared".into(), "local".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let after = std::fs::metadata(&global).unwrap().modified().unwrap();
        assert_eq!(
            before, after,
            "~/.claude/settings.json must not be touched by apply"
        );
    }

    #[test]
    #[serial(home_env)]
    fn apply_unknown_project_is_rejected() {
        let _g = setup_home();
        let profile = make_layered_profile("p", ProfileLayers::default());
        save_profile(profile).unwrap();

        let err = apply_profile_to_project(
            "p".into(),
            "/some/random/path".into(),
            ApplyOptions {
                layers: vec!["local".into()],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("not registered"));
    }

    #[test]
    #[serial(home_env)]
    fn apply_unknown_layer_name_errors() {
        let g = setup_home();
        let proj = make_project(g.path(), "demo");
        let proj_canonical = std::fs::canonicalize(&proj)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        add_project(proj.to_string_lossy().into_owned()).unwrap();

        let profile = make_layered_profile("work", ProfileLayers::default());
        save_profile(profile).unwrap();

        let err = apply_profile_to_project(
            "work".into(),
            proj_canonical,
            ApplyOptions {
                layers: vec!["bogus".into()],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(format!("{err}").contains("unknown layer"));
    }
}

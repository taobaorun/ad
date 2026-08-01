#![allow(clippy::result_large_err)]

// ad library entry — composes filesystem primitives, commands, and the tray.

pub mod agents;
pub mod fs;
mod migration;
mod models;
mod terminal;
mod tray;

pub mod commands;

use tauri::webview::{Color, PageLoadEvent};
use tauri::{Manager, WebviewWindowBuilder, WindowEvent};
use tracing::info;

fn theme_bg() -> Color {
    let is_dark = fs::paths::theme_hint_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() != "light")
        .unwrap_or(true);
    theme_bg_for(is_dark)
}

fn theme_bg_for(is_dark: bool) -> Color {
    if is_dark {
        Color(0x1e, 0x1e, 0x2e, 0xff)
    } else {
        Color(0xef, 0xf1, 0xf5, 0xff)
    }
}

fn should_show_main_on_page_load(label: &str, event: PageLoadEvent) -> bool {
    label == "main" && matches!(event, PageLoadEvent::Started)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(agents::PlanStore::default())
        .manage(agents::SkillCatalogPlanStore::default())
        .manage(commands::shortcut::ShortcutState::default())
        .setup(|app| {
            // First-run: relocate AD data from ~/.claude/ to ~/.ad/ if needed.
            // Must run before legacy profile migration since the latter walks
            // profiles_dir() which now points at ~/.ad/profiles/.
            match migration::migrate_data_dir_to_home() {
                Ok(true) => info!("migrated AD data from ~/.claude/ to ~/.ad/"),
                Ok(false) => {}
                Err(err) => tracing::warn!(?err, "data dir migration failed; continuing"),
            }

            // Convert any super-old { displayName, env } profiles to v1 shape.
            if let Err(err) = migration::migrate_legacy_profiles() {
                tracing::warn!(?err, "legacy profile migration failed; continuing");
            }

            // Convert v1 profiles (flat settings) to v2 layered shape.
            match migration::migrate_v1_profiles_to_layered() {
                Ok(0) => {}
                Ok(n) => info!(count = n, "migrated v1 profiles to layered shape"),
                Err(err) => tracing::warn!(?err, "v1 -> layered migration failed; continuing"),
            }

            match agents::recover_execution_state() {
                Ok(recovery) if recovery.writable() => {
                    info!(
                        inspected = recovery.inspected,
                        recovered = recovery.recovered,
                        "operation recovery completed"
                    );
                }
                Ok(recovery) => {
                    tracing::warn!(
                        inspected = recovery.inspected,
                        repair_required = recovery.repair_required,
                        diagnostics = ?recovery.diagnostics,
                        "operation recovery requires repair; Agent mutations are blocked"
                    );
                }
                Err(error) if recovery_can_be_deferred(&error) => {
                    tracing::warn!(
                        error = %error,
                        "operation recovery is held by another AD process; startup continues"
                    );
                }
                Err(error) => return Err(std::io::Error::other(error.to_string()).into()),
            }

            match agents::recover_skill_catalog_state() {
                Ok(recovery) if recovery.writable() => {
                    info!(
                        inspected = recovery.inspected,
                        recovered = recovery.recovered,
                        compensated = recovery.compensated,
                        removed_staging = recovery.removed_staging,
                        "Skill catalog recovery completed"
                    );
                }
                Ok(recovery) => {
                    tracing::warn!(
                        repair_required = recovery.repair_required,
                        diagnostics = ?recovery.diagnostics,
                        "Skill catalog recovery requires repair; catalog mutations are blocked"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Skill catalog recovery could not acquire a safe startup state"
                    );
                }
            }

            // Show the theme-backed main window as navigation starts so the
            // HTML spotlight represents the actual startup work from its first
            // available frame instead of appearing after loading has finished.
            let bg = theme_bg();
            WebviewWindowBuilder::from_config(app.handle(), &app.config().app.windows[0])?
                .background_color(bg)
                .visible(false)
                .on_page_load(|window, payload| {
                    if should_show_main_on_page_load(window.label(), payload.event()) {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                })
                .build()?;

            // Pre-create settings window hidden so it's instantly ready when
            // the user clicks the gear icon — no loading flash.
            // NOT a child of main — macOS shows child windows when the parent
            // becomes visible, which would flash settings on launch.
            WebviewWindowBuilder::new(
                app.handle(),
                "settings",
                tauri::WebviewUrl::App("index.html#/settings".into()),
            )
            .title("Settings")
            .inner_size(720.0, 520.0)
            .min_inner_size(560.0, 400.0)
            .resizable(true)
            .visible(false)
            .background_color(bg)
            .build()?;

            // Initialize menubar tray with the active profile color.
            tray::install(app.handle())?;

            // Register the default OS-global shortcut for show/hide. The
            // frontend will re-register on boot with the user's persisted
            // binding, so this is mainly a fallback for the first frame.
            commands::shortcut::register_default(app.handle(), commands::shortcut::DEFAULT_BINDING);

            info!("ad ready");
            Ok(())
        })
        .on_window_event(|window, event| {
            // Only the main window hides-on-close: ad is a menubar app and
            // the user expects it to stay running. Auxiliary windows
            // (e.g. settings) close normally so they can be re-created
            // from a fresh state on the next open.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" || window.label() == "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::profiles::list_profiles,
            commands::profiles::get_profile,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::profiles::get_active_profile_id,
            commands::profiles::list_agent_profiles,
            commands::profiles::get_agent_profile,
            commands::profiles::save_agent_profile,
            commands::profiles::delete_agent_profile,
            commands::profile_envelopes::list_profile_envelopes,
            commands::profile_envelopes::get_profile_envelope,
            commands::profile_envelopes::save_profile_envelope,
            commands::profile_envelopes::delete_profile_envelope,
            commands::settings::read_current_settings,
            commands::settings::write_theme_hint,
            commands::activate::activate_profile,
            commands::activate::detect_claude_processes,
            commands::agents::list_agents,
            commands::agents::discover_agents,
            commands::agents::list_agent_capabilities,
            commands::agents::resolve_agent_context,
            commands::agents::resolve_project_agent_workspace,
            commands::agents::inspect_project_agent_workspace,
            commands::agents::inspect_agent_settings,
            commands::agents::list_agent_settings_documents,
            commands::agents::list_agent_skills,
            commands::agents::list_agent_plugins,
            commands::agents::detect_agent_processes,
            commands::agents::inspect_project_codex_runtime,
            commands::agents::list_agent_operation_history,
            commands::agents::preview_claude_to_codex,
            commands::agents::preview_claude_to_codex_route,
            commands::agents::preview_agent_settings_edit,
            commands::agents::preview_agent_profile_apply,
            commands::agents::preview_agent_collection_install,
            commands::agents::preview_agent_collection_toggle,
            commands::agents::apply_agent_plan,
            commands::agents::apply_conversion_plan,
            commands::agents::preview_agent_rollback,
            commands::agents::apply_agent_rollback_plan,
            commands::history::read_history,
            commands::history::restore_backup,
            commands::importers::import_from_file,
            commands::importers::import_from_url,
            // M2: layered apply + project registry + auto-detect
            commands::projects::list_projects,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
            commands::projects::set_project_pinned,
            commands::projects::set_project_codex_config_inheritance,
            commands::projects::get_project_status,
            commands::projects::read_project_settings,
            commands::projects::write_project_settings,
            commands::apply::apply_profile_to_project,
            commands::scan_roots::list_scan_roots,
            commands::scan_roots::add_scan_root,
            commands::scan_roots::remove_scan_root,
            commands::scan_roots::set_scan_root_enabled,
            commands::discover::scan_for_projects,
            commands::path_complete::complete_path_prefix,
            commands::terminal::open_in_terminal,
            commands::terminal::list_terminal_backends,
            commands::shortcut::set_global_shortcut,
            commands::settings::open_settings_window,
            // Skill management
            commands::skills::list_skill_sources,
            commands::skills::add_skill_source,
            commands::skills::remove_skill_source,
            commands::skills::update_skill_source,
            commands::skills::scan_skill_library,
            commands::skills::get_project_skills,
            commands::skills::toggle_skill,
            commands::skills::set_skill_scope,
            commands::skills::apply_project_skills,
            commands::skills::list_plugins,
            commands::skills::toggle_plugin,
            commands::skill_catalog::list_skill_catalog,
            commands::skill_catalog::preview_add_skill_catalog_source,
            commands::skill_catalog::preview_update_skill_catalog_source,
            commands::skill_catalog::preview_remove_skill_catalog_source,
            commands::skill_catalog::apply_skill_catalog_source_plan,
            commands::skill_catalog::cancel_skill_catalog_source_plan,
            commands::skill_catalog::inspect_legacy_skill_state,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ad")
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        });
}

fn recovery_can_be_deferred(error: &agents::AgentError) -> bool {
    error.retryable
        && error
            .details
            .as_ref()
            .and_then(|details| details.get("phase"))
            .and_then(serde_json::Value::as_str)
            == Some("operation_recovery")
}

#[cfg(test)]
mod tests {
    use crate::agents;
    use tauri::webview::PageLoadEvent;

    use super::{recovery_can_be_deferred, should_show_main_on_page_load, theme_bg_for};

    #[test]
    fn maps_theme_mode_to_catppuccin_window_backgrounds() {
        let mocha = theme_bg_for(true);
        let latte = theme_bg_for(false);

        assert_eq!(
            (mocha.0, mocha.1, mocha.2, mocha.3),
            (0x1e, 0x1e, 0x2e, 0xff)
        );
        assert_eq!(
            (latte.0, latte.1, latte.2, latte.3),
            (0xef, 0xf1, 0xf5, 0xff)
        );
    }

    #[test]
    fn main_window_is_shown_as_navigation_starts() {
        assert!(should_show_main_on_page_load(
            "main",
            PageLoadEvent::Started
        ));
        assert!(!should_show_main_on_page_load(
            "main",
            PageLoadEvent::Finished
        ));
        assert!(!should_show_main_on_page_load(
            "settings",
            PageLoadEvent::Finished
        ));
    }

    #[test]
    fn only_retryable_recovery_lock_conflicts_defer_startup() {
        let deferred = agents::AgentError {
            code: agents::AgentErrorCode::ResourceChanged,
            message: "locked".into(),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: true,
            details: Some(serde_json::json!({"phase": "operation_recovery"})),
        };
        let fatal = agents::AgentError {
            retryable: false,
            ..deferred.clone()
        };

        assert!(recovery_can_be_deferred(&deferred));
        assert!(!recovery_can_be_deferred(&fatal));
    }
}

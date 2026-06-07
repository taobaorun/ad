// ad library entry — composes filesystem primitives, commands, and the tray.

pub mod fs;
mod migration;
mod models;
mod terminal;
mod tray;

pub mod commands;

use tauri::webview::Color;
use tauri::{Manager, WebviewWindowBuilder, WindowEvent};
use tracing::info;

fn theme_bg() -> Color {
    let is_dark = fs::paths::theme_hint_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() != "light")
        .unwrap_or(true);
    if is_dark {
        Color(0x0a, 0x0a, 0x0b, 0xff)
    } else {
        Color(0xff, 0xff, 0xff, 0xff)
    }
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

            // Create the main window hidden, with theme-aware background.
            // on_page_load(Finished) fires from WKWebView's didFinishNavigation
            // delegate — at that point the HTML/CSS splash is fully loaded and
            // composited, so the user sees the splash instead of a blank frame.
            let bg = theme_bg();
            let main_win = WebviewWindowBuilder::from_config(
                app.handle(),
                &app.config().app.windows[0],
            )?
            .background_color(bg)
            .visible(false)
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
            commands::shortcut::register_default(
                app.handle(),
                commands::shortcut::DEFAULT_BINDING,
            );

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
            commands::settings::read_current_settings,
            commands::settings::write_theme_hint,
            commands::activate::activate_profile,
            commands::activate::detect_claude_processes,
            commands::history::read_history,
            commands::history::restore_backup,
            commands::importers::import_from_file,
            commands::importers::import_from_url,
            // M2: layered apply + project registry + auto-detect
            commands::projects::list_projects,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::projects::rename_project,
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

// ad library entry — composes filesystem primitives, commands, and the tray.

pub mod fs;
mod migration;
mod models;
mod tray;

pub mod commands;

use tauri::WindowEvent;
use tracing::info;

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

            // Initialize menubar tray with the active profile color.
            tray::install(app.handle())?;

            info!("ad ready");
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window button hides instead of quitting — ad
            // is a menubar app, the user expects it to stay running. Quit
            // happens explicitly via the tray "Quit" menu item.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::profiles::list_profiles,
            commands::profiles::get_profile,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::profiles::get_active_profile_id,
            commands::settings::read_current_settings,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running ad");
}

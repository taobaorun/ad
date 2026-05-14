// cc-switch library entry — composes filesystem primitives, commands, and the tray.

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
            // First-run migration of legacy profile files.
            if let Err(err) = migration::migrate_legacy_profiles() {
                tracing::warn!(?err, "legacy migration failed; continuing");
            }

            // Initialize menubar tray with the active profile color.
            tray::install(app.handle())?;

            info!("cc-switch ready");
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window button hides instead of quitting — cc-switch
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running cc-switch");
}

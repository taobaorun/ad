//! Global (OS-level) keyboard shortcut for showing / hiding the main window.
//!
//! Backed by `tauri-plugin-global-shortcut`. Exposes a single re-registerable
//! binding (default `Alt+Cmd+KeyA`) that toggles the main window visibility,
//! mirroring the tray "Show AD" menu item but reachable from anywhere on the
//! OS. The currently-registered binding is tracked in `ShortcutState` so the
//! frontend can change it without restarting the app.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState as ShortcutEventState};

/// Default binding shipped with the app: `⌥⌘A`.
pub const DEFAULT_BINDING: &str = "Alt+Cmd+KeyA";

/// Holds the currently-registered shortcut string so we can unregister it
/// before registering a new one. `None` means no global shortcut is active.
#[derive(Default)]
pub struct ShortcutState {
    current: Mutex<Option<String>>,
}

/// Show the main window if hidden / unfocused, otherwise hide it. Matches the
/// Raycast / Alfred toggle pattern the user expects from menubar apps.
fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    if visible && focused {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Parse `binding` and register the toggle handler. Caller is responsible for
/// unregistering any prior binding first.
fn register<R: Runtime>(app: &AppHandle<R>, binding: &str) -> Result<(), String> {
    let shortcut: Shortcut = binding
        .parse()
        .map_err(|e| format!("invalid shortcut '{binding}': {e}"))?;
    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() == ShortcutEventState::Pressed {
                toggle_main_window(&app_handle);
            }
        })
        .map_err(|e| format!("register failed: {e}"))?;
    Ok(())
}

fn unregister<R: Runtime>(app: &AppHandle<R>, binding: &str) -> Result<(), String> {
    let shortcut: Shortcut = binding
        .parse()
        .map_err(|e| format!("invalid shortcut '{binding}': {e}"))?;
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("unregister failed: {e}"))
}

/// Called once at app boot from `lib.rs::setup`. Failure is logged but not
/// propagated — a missing shortcut shouldn't block the app from launching.
pub fn register_default<R: Runtime>(app: &AppHandle<R>, binding: &str) {
    if let Err(err) = register(app, binding) {
        tracing::warn!(?err, binding, "failed to register default global shortcut");
        return;
    }
    let state = app.state::<ShortcutState>();
    let mut guard = match state.current.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    *guard = Some(binding.to_string());
}

/// IPC command: replace the active shortcut binding.
///
/// - `Some(binding)` — unregister current (if any) and register the new one.
/// - `None` — unregister current (if any); leaves no shortcut active.
///
/// Returns `Ok(())` on success; error string is surfaced to the frontend toast.
#[tauri::command]
pub async fn set_global_shortcut<R: Runtime>(
    app: AppHandle<R>,
    binding: Option<String>,
) -> Result<(), String> {
    let state = app.state::<ShortcutState>();

    let previous = state
        .current
        .lock()
        .map_err(|_| "shortcut state poisoned".to_string())?
        .clone();

    if let Some(prev) = previous.as_deref() {
        if let Err(err) = unregister(&app, prev) {
            tracing::warn!(?err, prev, "failed to unregister previous shortcut");
        }
    }

    match binding.as_deref() {
        Some(next) => {
            register(&app, next)?;
            *state
                .current
                .lock()
                .map_err(|_| "shortcut state poisoned".to_string())? = Some(next.to_string());
        }
        None => {
            *state
                .current
                .lock()
                .map_err(|_| "shortcut state poisoned".to_string())? = None;
        }
    }

    Ok(())
}

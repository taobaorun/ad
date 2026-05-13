//! macOS menubar tray. Shows a colored dot for the active profile and a menu of
//! all profiles. Clicking a profile activates it via the same code path as the
//! main UI.

mod icon;

use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

use crate::commands::profiles::{get_active_profile_id, list_profiles};

pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let menu = build_menu(app)?;
    let icon_bytes = current_icon_bytes()?;

    let _ = TrayIconBuilder::with_id("cc-switch-tray")
        .icon(tauri::image::Image::from_bytes(&icon_bytes)?)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>> {
    let profiles = list_profiles().unwrap_or_default();
    let active = get_active_profile_id().ok().flatten();

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();
    for p in &profiles {
        let mark = if Some(&p.id) == active.as_ref() {
            "● "
        } else {
            "○ "
        };
        let label = format!("{mark}{}", p.display_name);
        let item = MenuItem::with_id(app, format!("activate:{}", p.id), label, true, None::<&str>)?;
        items.push(Box::new(item));
    }

    if !profiles.is_empty() {
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }
    let show = MenuItem::with_id(app, "show-main", "Show cc-switch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    items.push(Box::new(show));
    items.push(Box::new(quit));

    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items.iter().map(|b| b.as_ref()).collect();
    Ok(Menu::with_items(app, &refs)?)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == "quit" {
        app.exit(0);
        return;
    }
    if id == "show-main" {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return;
    }
    if let Some(profile_id) = id.strip_prefix("activate:") {
        let pid = profile_id.to_string();
        let app_clone = app.clone();
        std::thread::spawn(move || {
            if let Err(err) = crate::commands::activate::activate_profile(pid) {
                tracing::warn!(?err, "tray-driven activation failed");
            }
            // Rebuild the menu so the active marker moves.
            if let Ok(menu) = build_menu(&app_clone) {
                if let Some(tray) = app_clone.tray_by_id("cc-switch-tray") {
                    let _ = tray.set_menu(Some(menu));
                    if let Ok(bytes) = current_icon_bytes() {
                        let _ =
                            tray.set_icon(Some(tauri::image::Image::from_bytes(&bytes).unwrap()));
                    }
                }
            }
        });
    }
}

fn current_icon_bytes() -> Result<Vec<u8>> {
    let active = get_active_profile_id().ok().flatten();
    let color = match active.as_deref() {
        Some(id) => list_profiles()
            .ok()
            .and_then(|ps| ps.into_iter().find(|p| p.id == id))
            .map(|p| p.color)
            .unwrap_or_else(|| "#7C3AED".to_string()),
        None => "#9CA3AF".to_string(),
    };
    icon::for_color(&color)
}

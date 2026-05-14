//! macOS menubar tray. Shows a colored dot for the active profile and a menu of
//! all profiles. Clicking a profile activates it via the same code path as the
//! main UI.

mod icon;

use anyhow::Result;
use tauri::{
    menu::{IconMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Listener, Manager, Runtime,
};

use crate::commands::activate::PROFILE_ACTIVATED_EVENT;
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

    // Listen for `profile-activated` events from anywhere (UI-driven activation,
    // tray menu activation, or a future programmatic source) and refresh the
    // tray icon + menu. Without this, activations from the main window leave
    // the tray showing the previous active profile.
    let app_handle = app.clone();
    app.listen(PROFILE_ACTIVATED_EVENT, move |_event| {
        refresh(&app_handle);
    });

    Ok(())
}

/// Rebuild the tray's menu + icon from the current on-disk state.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id("cc-switch-tray") else {
        return;
    };
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    if let Ok(bytes) = current_icon_bytes() {
        if let Ok(image) = tauri::image::Image::from_bytes(&bytes) {
            let _ = tray.set_icon(Some(image));
        }
    }
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>> {
    let profiles = list_profiles().unwrap_or_default();
    let active = get_active_profile_id().ok().flatten();

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();
    for p in &profiles {
        let is_active = Some(&p.id) == active.as_ref();
        // Active profile: filled colored circle. Inactive: outlined ring of
        // the same color. Both render as a real icon in the menu so it's
        // immediately obvious which one is current and what color each is.
        let icon_bytes = if is_active {
            icon::for_color_filled(&p.color, icon::MENU_ITEM_SIZE)
        } else {
            icon::for_color_ring(&p.color, icon::MENU_ITEM_SIZE)
        }?;
        let icon_image = tauri::image::Image::from_bytes(&icon_bytes)?;
        // Append a checkmark to the active label as a redundant accessibility
        // cue; the icon already carries the color, but VoiceOver users get the
        // text affordance.
        let label = if is_active {
            format!("{}  ✓", p.display_name)
        } else {
            p.display_name.clone()
        };
        let item = IconMenuItem::with_id(
            app,
            format!("activate:{}", p.id),
            label,
            true,
            Some(icon_image),
            None::<&str>,
        )?;
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
        // Spawn so the menu callback returns quickly. The activation itself
        // is sync; the `profile-activated` event we emit afterward fans out
        // to the tray listener (which rebuilds icon+menu) and the frontend
        // listener (which refreshes the store).
        std::thread::spawn(
            move || match crate::commands::activate::activate_profile_inner(pid) {
                Ok(result) => crate::commands::activate::emit_activated(&app_clone, &result),
                Err(err) => tracing::warn!(?err, "tray-driven activation failed"),
            },
        );
    }
}

fn current_icon_bytes() -> Result<Vec<u8>> {
    let active = get_active_profile_id().ok().flatten();
    // Ring color tracks the active profile. Brand purple #7C3AED is the
    // fallback when no profile is selected so the tray is never colorless.
    let ring_color = match active.as_deref() {
        Some(id) => list_profiles()
            .ok()
            .and_then(|ps| ps.into_iter().find(|p| p.id == id))
            .map(|p| p.color)
            .unwrap_or_else(|| "#7C3AED".to_string()),
        None => "#7C3AED".to_string(),
    };
    icon::for_brand_with_ring(&ring_color)
}

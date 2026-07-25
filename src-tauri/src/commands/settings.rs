use crate::fs::paths::{claude_settings_path, ensure_dir, theme_hint_path};
use crate::models::ClaudeSettings;

use super::CmdResult;

#[tauri::command]
pub fn read_current_settings() -> CmdResult<Option<ClaudeSettings>> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let settings: ClaudeSettings = serde_json::from_slice(&bytes)?;
    Ok(Some(settings))
}

#[tauri::command]
pub fn open_settings_window(app: tauri::AppHandle) -> CmdResult<()> {
    use tauri::Manager;
    let win = app
        .get_webview_window("settings")
        .ok_or_else(|| super::CommandError::Generic("settings window not found".into()))?;
    let _ = win.show();
    let _ = win.set_focus();
    // WKWebView skips paint for hidden windows — force a reflow so the
    // pre-rendered content actually appears.
    let _ = win.eval(
        "requestAnimationFrame(function(){document.body.style.display='none';\
         void document.body.offsetHeight;\
         document.body.style.display=''})",
    );
    Ok(())
}

#[tauri::command]
pub fn write_theme_hint(dark: bool) -> CmdResult<()> {
    let path = theme_hint_path()?;
    ensure_dir(path.parent().unwrap())?;
    std::fs::write(&path, if dark { "dark" } else { "light" })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial(home_env)]
    fn returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());
        assert!(read_current_settings().unwrap().is_none());
    }

    #[test]
    #[serial(home_env)]
    fn parses_existing_file() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AD_HOME", tmp.path());
        let p = claude_settings_path().unwrap();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, br#"{"env":{"K":"V"}}"#).unwrap();
        let s = read_current_settings().unwrap().unwrap();
        assert_eq!(s.env.get("K").map(String::as_str), Some("V"));
    }
}

use crate::fs::paths::claude_settings_path;
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

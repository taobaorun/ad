use std::path::PathBuf;

use serde::Serialize;

use crate::terminal::{self, LaunchSpec, TerminalBackend};

use super::CmdResult;

#[derive(Serialize)]
pub struct TerminalBackendInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[tauri::command]
pub fn list_terminal_backends() -> Vec<TerminalBackendInfo> {
    vec![
        TerminalBackendInfo {
            id: "ghostty",
            label: TerminalBackend::Ghostty.label(),
        },
        TerminalBackendInfo {
            id: "cmux",
            label: TerminalBackend::Cmux.label(),
        },
        TerminalBackendInfo {
            id: "apple-terminal",
            label: TerminalBackend::AppleTerminal.label(),
        },
        TerminalBackendInfo {
            id: "custom",
            label: TerminalBackend::Custom.label(),
        },
    ]
}

#[tauri::command]
pub fn open_in_terminal(
    project_path: String,
    backend: String,
    claude_bin: Option<String>,
    custom_template: Option<String>,
) -> CmdResult<()> {
    let backend = TerminalBackend::from_str_lossy(&backend)
        .map_err(|e| super::CommandError::Generic(e.to_string()))?;
    let cwd = PathBuf::from(project_path);
    let claude = claude_bin.as_deref().filter(|s| !s.is_empty()).unwrap_or("claude");
    let template = custom_template.as_deref().filter(|s| !s.is_empty());
    let spec = LaunchSpec {
        cwd: &cwd,
        claude_bin: claude,
        custom_template: template,
    };
    terminal::launch(backend, spec).map_err(Into::into)
}

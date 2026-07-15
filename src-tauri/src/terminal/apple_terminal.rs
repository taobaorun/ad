// Terminal.app launcher.
//
// Strategy is symmetric to Ghostty's:
//   1. If Terminal.app is running, try an AppleScript that opens a new tab in
//      the front window and `do script` the cd+Agent command.
//   2. Otherwise fall back to launching a fresh window via AppleScript
//      `do script` against application "Terminal" (which auto-launches).
//
// `do script` in Terminal already opens a new window if no `in window …`
// target is supplied, so the fallback is naturally simple.

use std::process::Command;

use anyhow::{anyhow, Result};

use super::{escape_applescript_string, shell_quote, LaunchSpec};

pub fn launch(spec: &LaunchSpec<'_>) -> Result<()> {
    let cwd = spec
        .cwd
        .to_str()
        .ok_or_else(|| anyhow!("non-utf8 project path"))?;
    let shell_cmd = format!("cd {} && {}", shell_quote(cwd), spec.command());
    let escaped = escape_applescript_string(&shell_cmd);

    let script = if terminal_is_running() {
        format!(
            r#"tell application "Terminal"
    activate
    tell application "System Events" to keystroke "t" using command down
    delay 0.15
    do script "{cmd}" in front window
end tell"#,
            cmd = escaped
        )
    } else {
        format!(
            r#"tell application "Terminal"
    activate
    do script "{cmd}"
end tell"#,
            cmd = escaped
        )
    };

    let status = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .map_err(|e| anyhow!("failed to spawn osascript: {e}"))?;
    if !status.success() {
        return Err(anyhow!("osascript exited with {status}"));
    }
    Ok(())
}

fn terminal_is_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "Terminal"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// cmux launcher.
//
// cmux exposes a Unix-socket IPC. Two paths:
//
//   * cmux running: `cmux new-workspace --cwd <path> --command <cmd>` creates
//     a workspace (tab) in the caller's window and types <cmd>+Enter into it.
//
//   * cmux not running: `cmux <path>` is the canonical bootstrap — it spawns
//     the app, waits for the socket, and creates the first workspace. It
//     prints `OK <workspace_ref>` to stdout. We capture that ref and then
//     send `<cmd>\n` to that workspace via `cmux send --workspace <ref> --
//     <cmd>\n` so the user ends up with exactly one workspace containing
//     a running Agent.

use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use super::{resolve_bin, LaunchSpec};

const SOCKET_REL: &str = "Library/Application Support/cmux/cmux.sock";

pub fn launch(spec: &LaunchSpec<'_>) -> Result<()> {
    let cwd = spec
        .cwd
        .to_str()
        .ok_or_else(|| anyhow!("non-utf8 project path"))?;
    let cmux = resolve_bin("cmux").ok_or_else(|| {
        anyhow!("cmux binary not found in your login shell PATH — install cmux and ensure `cmux` is on PATH")
    })?;

    if cmux_socket_exists() {
        return new_workspace_with_command(&cmux, cwd, &spec.command());
    }

    let ws_ref = bootstrap_cmux(&cmux, cwd)?;
    thread::sleep(Duration::from_millis(400));
    send_text_to_workspace(&cmux, &ws_ref, &format!("{}\n", spec.command()))
}

fn cmux_socket_exists() -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        std::path::Path::new(&home).join(SOCKET_REL).exists()
    } else {
        false
    }
}

fn bootstrap_cmux(cmux: &str, cwd: &str) -> Result<String> {
    let output = Command::new(cmux)
        .arg(cwd)
        .output()
        .map_err(|e| anyhow!("failed to spawn `cmux`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(maybe_explain_access_denied(&format!(
            "`cmux {cwd}` exited with {} — {stderr}",
            output.status
        )));
    }
    parse_workspace_ref(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| anyhow!("cmux did not return a workspace ref on stdout"))
}

fn parse_workspace_ref(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("OK ") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn new_workspace_with_command(cmux: &str, cwd: &str, command: &str) -> Result<()> {
    let output = Command::new(cmux)
        .args([
            "new-workspace",
            "--cwd",
            cwd,
            "--command",
            command,
            "--focus",
            "true",
        ])
        .output()
        .map_err(|e| anyhow!("failed to spawn `cmux new-workspace`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(maybe_explain_access_denied(&format!(
            "`cmux new-workspace` exited with {} — {stderr}",
            output.status
        )));
    }
    Ok(())
}

fn send_text_to_workspace(cmux: &str, ws_ref: &str, text: &str) -> Result<()> {
    let output = Command::new(cmux)
        .args(["send", "--workspace", ws_ref, "--", text])
        .output()
        .map_err(|e| anyhow!("failed to spawn `cmux send`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(maybe_explain_access_denied(&format!(
            "`cmux send` exited with {} — {stderr}",
            output.status
        )));
    }
    Ok(())
}

/// cmux's `cmuxOnly` access mode (the default) blocks external automation
/// clients like AD. When we see that error, append a hint pointing the user
/// at the cmux setting they need to change.
fn maybe_explain_access_denied(msg: &str) -> anyhow::Error {
    if msg.contains("Access denied") || msg.contains("only processes started inside cmux") {
        anyhow!(
            "{msg}\n\nFix: open cmux Settings → Socket Control and switch the access mode to \"Automation mode\" (recommended) or \"Full open access\". AD runs as an external automation client and is blocked by the default \"cmux processes only\" mode."
        )
    } else {
        anyhow!("{msg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_ref_from_stdout() {
        assert_eq!(
            parse_workspace_ref("OK workspace:1\n").as_deref(),
            Some("workspace:1")
        );
        assert_eq!(
            parse_workspace_ref("noise\nOK abc-123\nmore\n").as_deref(),
            Some("abc-123")
        );
        assert_eq!(parse_workspace_ref("").as_deref(), None);
        assert_eq!(parse_workspace_ref("OK\n").as_deref(), None);
    }
}

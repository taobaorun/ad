// Ghostty launcher.
//
// macOS Ghostty exposes its CLI via the .app bundle and accepts `-e <cmd>`
// to run a command in a new window. Two macOS-specific gotchas:
//
//   1. `-e <agent>` can fail with "No such file or directory" because Ghostty's
//      `-e` exec'd child does NOT source the user's shell rc, so PATH only
//      contains the system defaults. We must hand it an absolute path.
//      resolver queries the user's login shell to find the Agent executable.
//
//   2. `+new-window` is not supported on macOS, so reopening into a new tab
//      of an existing window would normally require `System Events` keystroke
//      automation — which needs Accessibility permission for whichever
//      binary spawns osascript (in our case osascript itself, which can't be
//      added to the Accessibility list cleanly). Until we ship a native
//      AppleScript bridge, the simplest correct behavior is: always
//      `open -na Ghostty.app ... -e <agent>`. macOS will reuse the running
//      Ghostty.app process (no new dock icon) but opens a fresh window.
//      Users who want tab reuse can fall back to cmux, which manages its own
//      workspace model.

use std::process::Command;

use anyhow::{anyhow, Result};

use super::{render_launch_command, resolve_bin, LaunchSpec};

pub fn launch(spec: &LaunchSpec<'_>) -> Result<()> {
    let cwd = spec
        .cwd
        .to_str()
        .ok_or_else(|| anyhow!("non-utf8 project path"))?;
    let program = resolve_bin(spec.program).ok_or_else(|| {
        anyhow!(
            "Agent binary not found in the login shell PATH: `{}`",
            spec.program
        )
    })?;

    // We wrap with `zsh -lc "cd <cwd> && exec <agent>"` for two reasons:
    //   1. Ghostty's `-e` exec'd child does not honor `--working-directory=`,
    //      so we must `cd` ourselves inside the shell before launching the Agent.
    //   2. The login shell sources the user's rc files first, giving the Agent
    //      the same environment the user would see in their normal terminal.
    let shell_cmd = format!(
        "cd {} && exec {}",
        super::shell_quote(cwd),
        render_launch_command(&program, spec.args, spec.env),
    );

    let status = Command::new("open")
        .args([
            "-na",
            "Ghostty.app",
            "--args",
            "-e",
            "/bin/zsh",
            "-lc",
            &shell_cmd,
        ])
        .status()
        .map_err(|e| anyhow!("failed to spawn `open` for Ghostty: {e}"))?;
    if !status.success() {
        return Err(anyhow!(
            "`open -na Ghostty.app` exited with {status} — is Ghostty installed?"
        ));
    }
    Ok(())
}

/// If `bin` is already absolute, return it as-is. Otherwise query the user's
/// login shell to resolve it through their PATH (so an Agent installed via
/// homebrew / nvm / asdf is found even though Ghostty's `-e` child won't
/// source rc files).
#[cfg(test)]
fn resolve_agent_path(bin: &str) -> Option<String> {
    resolve_bin(bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_absolute_path_returns_as_is() {
        assert_eq!(
            resolve_agent_path("/opt/homebrew/bin/claude"),
            Some("/opt/homebrew/bin/claude".to_string())
        );
    }

    #[test]
    fn resolve_finds_common_binary_through_login_shell() {
        // `ls` is always on PATH; using it as a proxy to verify the
        // login-shell lookup mechanism without depending on claude being
        // installed in the test environment.
        let p = resolve_agent_path("ls").expect("ls should resolve");
        assert!(p.starts_with('/'), "expected absolute path, got: {p}");
        assert!(p.ends_with("/ls"), "expected ls binary, got: {p}");
    }

    #[test]
    fn resolve_missing_binary_returns_none() {
        assert!(resolve_agent_path("definitely-not-a-real-binary-xyz-12345").is_none());
    }

    #[test]
    fn resolve_treats_program_name_as_data() {
        assert!(resolve_agent_path("ls; true").is_none());
    }
}

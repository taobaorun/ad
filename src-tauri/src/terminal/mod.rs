// External terminal launcher abstraction.
//
// Backends are a closed set; enum dispatch is simpler than trait objects and
// plays nicely with serde for IPC. Each backend implementation lives in its
// own file and exposes a single `launch` function consumed by `launch()` below.

use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub mod apple_terminal;
pub mod cmux;
pub mod custom;
pub mod ghostty;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalBackend {
    Ghostty,
    Cmux,
    AppleTerminal,
    Custom,
}

impl TerminalBackend {
    pub fn from_str_lossy(s: &str) -> Result<Self> {
        match s {
            "ghostty" => Ok(Self::Ghostty),
            "cmux" => Ok(Self::Cmux),
            "apple-terminal" | "terminal" | "apple_terminal" => Ok(Self::AppleTerminal),
            "custom" => Ok(Self::Custom),
            other => Err(anyhow!("unknown terminal backend: {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ghostty => "Ghostty",
            Self::Cmux => "cmux",
            Self::AppleTerminal => "Terminal.app",
            Self::Custom => "Custom",
        }
    }
}

pub struct LaunchSpec<'a> {
    pub cwd: &'a Path,
    pub claude_bin: &'a str,
    pub custom_template: Option<&'a str>,
}

pub fn launch(backend: TerminalBackend, spec: LaunchSpec<'_>) -> Result<()> {
    if !spec.cwd.is_dir() {
        return Err(anyhow!(
            "project path is not a directory: {}",
            spec.cwd.display()
        ));
    }
    match backend {
        TerminalBackend::Ghostty => ghostty::launch(&spec),
        TerminalBackend::Cmux => cmux::launch(&spec),
        TerminalBackend::AppleTerminal => apple_terminal::launch(&spec),
        TerminalBackend::Custom => custom::launch(&spec),
    }
}

/// Escape a string for safe embedding inside a single-quoted AppleScript
/// literal. AppleScript single-quoted strings have no escape sequences, so we
/// close the quote, emit a literal quote via `& "'" &`, and reopen.
pub(crate) fn escape_applescript_string(s: &str) -> String {
    // AppleScript double-quoted strings allow `\"` and `\\`.
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// Shell-escape (POSIX single-quote style) for use inside backtick/sh -c
/// invocations or AppleScript `do shell script` payloads.
pub(crate) fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Resolve a binary name to an absolute path by querying the user's login
/// shell. Needed because Tauri/GUI apps launched from the Dock or Launchpad
/// inherit only the system PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), missing
/// homebrew (`/opt/homebrew/bin`), nvm, asdf, etc. If `bin` is already
/// absolute, it's returned as-is.
pub(crate) fn resolve_bin(bin: &str) -> Option<String> {
    if bin.starts_with('/') {
        return Some(bin.to_string());
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = std::process::Command::new(&shell)
        .args(["-lc", &format!("command -v {bin}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_from_str() {
        assert_eq!(
            TerminalBackend::from_str_lossy("ghostty").unwrap(),
            TerminalBackend::Ghostty
        );
        assert_eq!(
            TerminalBackend::from_str_lossy("terminal").unwrap(),
            TerminalBackend::AppleTerminal
        );
        assert!(TerminalBackend::from_str_lossy("wezterm").is_err());
    }

    #[test]
    fn applescript_escape_handles_quotes_and_backslashes() {
        assert_eq!(escape_applescript_string("foo"), "foo");
        assert_eq!(escape_applescript_string("a\"b"), "a\\\"b");
        assert_eq!(escape_applescript_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("foo bar"), "'foo bar'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }
}

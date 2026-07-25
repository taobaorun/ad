//! Git operations via the user's login shell.
//!
//! Tauri `.app` bundles launched from Finder/Dock inherit only the system
//! PATH and miss SSH_AUTH_SOCK, credential helpers, GPG agents, etc.
//! Running git through `zsh -lc "..."` sources the user's rc files first,
//! giving git the same auth context the user sees in their terminal.

use std::path::Path;
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};

fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

fn run_git_via_shell(args: &str, cwd: Option<&Path>) -> Result<Output> {
    let shell = login_shell();
    let mut cmd = Command::new(&shell);
    cmd.args(["-lc", &format!("git {args}")]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output()
        .with_context(|| format!("failed to run `{shell} -lc \"git {args}\"`"))
}

/// Test whether git is available and can authenticate against a remote.
/// Returns the resolved git path and the remote HEAD ref.
pub fn probe_git(remote_url: &str) -> Result<ProbeResult> {
    let which = run_git_via_shell("--version", None)?;
    if !which.status.success() {
        return Err(anyhow!("git not found in login shell PATH"));
    }
    let version = String::from_utf8_lossy(&which.stdout).trim().to_string();

    let ls = run_git_via_shell(&format!("ls-remote --heads {remote_url}"), None)?;
    let auth_ok = ls.status.success();
    let stderr = String::from_utf8_lossy(&ls.stderr).trim().to_string();

    Ok(ProbeResult {
        git_version: version,
        auth_ok,
        error: if auth_ok { None } else { Some(stderr) },
    })
}

/// Clone a repo into `dest`. Shallow clone (depth=1) by default.
pub fn clone(url: &str, dest: &Path, branch: Option<&str>) -> Result<()> {
    let dest_str = dest.to_str().ok_or_else(|| anyhow!("non-utf8 dest path"))?;
    let branch_flag = branch
        .map(|b| format!(" --branch '{b}'"))
        .unwrap_or_default();
    let cmd = format!("clone --depth 1{branch_flag} '{url}' '{dest_str}'");
    let out = run_git_via_shell(&cmd, None)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("git clone failed: {stderr}"));
    }
    Ok(())
}

/// Pull latest changes in an existing repo.
pub fn pull(repo_dir: &Path) -> Result<PullResult> {
    let before = run_git_via_shell("rev-parse HEAD", Some(repo_dir))?;
    let before_hash = String::from_utf8_lossy(&before.stdout).trim().to_string();

    let out = run_git_via_shell("pull --ff-only", Some(repo_dir))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("git pull failed: {stderr}"));
    }

    let after = run_git_via_shell("rev-parse HEAD", Some(repo_dir))?;
    let after_hash = String::from_utf8_lossy(&after.stdout).trim().to_string();

    Ok(PullResult {
        updated: before_hash != after_hash,
        before: before_hash,
        after: after_hash,
    })
}

/// Get the current HEAD commit hash of a repo.
pub fn head_hash(repo_dir: &Path) -> Result<String> {
    let out = run_git_via_shell("rev-parse --short HEAD", Some(repo_dir))?;
    if !out.status.success() {
        return Err(anyhow!("not a git repo: {}", repo_dir.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub git_version: String,
    pub auth_ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PullResult {
    pub updated: bool,
    pub before: String,
    pub after: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_version_reachable() {
        let out = run_git_via_shell("--version", None).unwrap();
        assert!(out.status.success());
        let ver = String::from_utf8_lossy(&out.stdout);
        assert!(ver.contains("git version"), "unexpected: {ver}");
    }

    #[test]
    #[ignore] // network-dependent
    fn probe_public_repo() {
        let result = probe_git("https://github.com/anthropics/anthropic-sdk-python.git").unwrap();
        assert!(
            result.auth_ok,
            "public repo should be accessible: {:?}",
            result.error
        );
    }

    #[test]
    #[ignore] // network-dependent
    fn probe_invalid_url() {
        let result =
            probe_git("https://github.com/nonexistent/repo-does-not-exist-12345.git").unwrap();
        assert!(!result.auth_ok);
        assert!(result.error.is_some());
    }

    #[test]
    #[ignore] // network-dependent
    fn clone_and_head_hash() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("test-repo");
        clone(
            "https://github.com/anthropics/anthropic-sdk-python.git",
            &dest,
            None,
        )
        .unwrap();
        assert!(dest.join(".git").is_dir());
        let hash = head_hash(&dest).unwrap();
        assert!(!hash.is_empty());
        assert!(hash.len() >= 7);
    }
}

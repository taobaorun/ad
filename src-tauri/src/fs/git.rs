//! Git execution with a fixed login-shell bootstrap and structured arguments.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use rustix::process::{kill_process_group, Pid, Signal};
use url::Url;

const LOGIN_SHELL: &str = "/bin/zsh";
const BOOTSTRAP_COMMAND: &str = "printf '__AD_GIT__=%s\\0' \"$(command -v git)\"; /usr/bin/env -0";
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);

const ENV_ALLOWLIST: &[&str] = &[
    "DISPLAY",
    "GCM_INTERACTIVE",
    "GIT_ASKPASS",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "HOME",
    "LANG",
    "LC_ALL",
    "LOGNAME",
    "PATH",
    "SSH_AGENT_PID",
    "SSH_ASKPASS",
    "SSH_AUTH_SOCK",
    "TMPDIR",
    "USER",
    "XDG_CONFIG_HOME",
];

#[derive(Debug, Clone, Copy)]
pub struct GitExecutionPolicy {
    pub total_timeout: Duration,
    pub no_progress_timeout: Duration,
}

impl Default for GitExecutionPolicy {
    fn default() -> Self {
        Self {
            total_timeout: Duration::from_secs(5 * 60),
            no_progress_timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
struct TrustedGit {
    executable: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    policy: GitExecutionPolicy,
}

impl TrustedGit {
    fn discover() -> Result<Self> {
        let output = Command::new(LOGIN_SHELL)
            .args(["-lc", BOOTSTRAP_COMMAND])
            .env("AD_GIT_BOOTSTRAP", "1")
            .output()
            .context("failed to bootstrap Git from the login shell")?;
        if !output.status.success() {
            return Err(anyhow!("login shell could not resolve Git"));
        }
        let values = parse_nul_environment(&output.stdout);
        let raw_git = values
            .get(OsStr::new("__AD_GIT__"))
            .ok_or_else(|| anyhow!("login shell returned no Git executable"))?;
        let executable = PathBuf::from(raw_git);
        if !executable.is_absolute() {
            return Err(anyhow!(
                "login shell returned a non-absolute Git executable"
            ));
        }
        let executable = std::fs::canonicalize(&executable).with_context(|| {
            format!("failed to resolve Git executable {}", executable.display())
        })?;
        if !executable.is_file() {
            return Err(anyhow!("resolved Git executable is not a file"));
        }
        let environment = values
            .into_iter()
            .filter(|(key, _)| key.to_str().is_some_and(|key| ENV_ALLOWLIST.contains(&key)))
            .collect();
        Ok(Self {
            executable,
            environment,
            policy: GitExecutionPolicy::default(),
        })
    }

    fn run<I, S>(&self, args: I, cwd: Option<&Path>) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.executable);
        command
            .args(args)
            .env_clear()
            .envs(&self.environment)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_LFS_SKIP_SMUDGE", "1")
            .env("GIT_CONFIG_COUNT", "3")
            .env("GIT_CONFIG_KEY_0", "submodule.recurse")
            .env("GIT_CONFIG_VALUE_0", "false")
            .env("GIT_CONFIG_KEY_1", "fetch.recurseSubmodules")
            .env("GIT_CONFIG_VALUE_1", "false")
            .env("GIT_CONFIG_KEY_2", "protocol.file.allow")
            .env("GIT_CONFIG_VALUE_2", "never")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(directory) = cwd {
            command.current_dir(directory);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to run {}", self.executable.display()))?;
        let started = Instant::now();
        let progress = Arc::new(AtomicU64::new(0));
        let stdout = drain_output(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("Git stdout pipe is unavailable"))?,
            Arc::clone(&progress),
            started,
        );
        let stderr = drain_output(
            child
                .stderr
                .take()
                .ok_or_else(|| anyhow!("Git stderr pipe is unavailable"))?,
            Arc::clone(&progress),
            started,
        );
        let status = loop {
            if let Some(status) = child.try_wait().context("failed to poll Git process")? {
                break status;
            }
            let elapsed = started.elapsed();
            let last_progress = Duration::from_millis(progress.load(Ordering::Relaxed));
            if elapsed > self.policy.total_timeout
                || elapsed.saturating_sub(last_progress) > self.policy.no_progress_timeout
            {
                terminate_process_group(&mut child);
                let reason = if elapsed > self.policy.total_timeout {
                    "total timeout"
                } else {
                    "no-progress timeout"
                };
                return Err(anyhow!("Git operation exceeded its {reason}"));
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        let stdout = stdout
            .join()
            .map_err(|_| anyhow!("Git stdout reader failed"))?;
        let stderr = stderr
            .join()
            .map_err(|_| anyhow!("Git stderr reader failed"))?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

fn parse_nul_environment(bytes: &[u8]) -> BTreeMap<OsString, OsString> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let separator = entry.iter().position(|byte| *byte == b'=')?;
            Some((
                OsString::from(String::from_utf8_lossy(&entry[..separator]).into_owned()),
                OsString::from(String::from_utf8_lossy(&entry[separator + 1..]).into_owned()),
            ))
        })
        .collect()
}

fn drain_output<R: Read + Send + 'static>(
    mut reader: R,
    progress: Arc<AtomicU64>,
    started: Instant,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    progress.store(
                        started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                        Ordering::Relaxed,
                    );
                    let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..read.min(remaining)]);
                }
            }
        }
        captured
    })
}

fn terminate_process_group(child: &mut std::process::Child) {
    let pid = Pid::from_child(child);
    let _ = kill_process_group(pid, Signal::TERM);
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let _ = kill_process_group(pid, Signal::KILL);
    let _ = child.wait();
}

fn validate_remote_url(remote_url: &str) -> Result<()> {
    if remote_url.is_empty()
        || remote_url.starts_with('-')
        || remote_url.chars().any(|character| character.is_control())
        || remote_url.contains([' ', ';', '|', '&', '`', '$'])
    {
        return Err(anyhow!("invalid Git remote URL"));
    }
    if let Ok(url) = Url::parse(remote_url) {
        if !matches!(url.scheme(), "https" | "ssh")
            || url.host_str().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(anyhow!("Git remote must use HTTPS or SSH"));
        }
        if url.password().is_some() || (url.scheme() == "https" && !url.username().is_empty()) {
            return Err(anyhow!(
                "Git credentials must use SSH agent or a credential helper"
            ));
        }
        return Ok(());
    }
    let Some((user_host, path)) = remote_url.split_once(':') else {
        return Err(anyhow!("invalid Git remote URL"));
    };
    if !user_host.contains('@')
        || user_host.contains(['/', '\\', ' ', ';', '|', '&', '`'])
        || path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|component| component == "..")
        || path.contains([' ', ';', '|', '&', '`', '$'])
    {
        return Err(anyhow!("invalid SSH Git remote URL"));
    }
    Ok(())
}

fn validate_ref(reference: &str) -> Result<()> {
    let valid_characters = reference
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character));
    if reference.is_empty()
        || reference.starts_with(['-', '/', '.'])
        || reference.ends_with(['/', '.'])
        || reference.contains("..")
        || reference.contains("//")
        || reference.contains("@{")
        || !valid_characters
    {
        return Err(anyhow!("invalid Git ref"));
    }
    Ok(())
}

fn checked_output(output: Output, operation: &str) -> Result<Output> {
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(anyhow!("{operation} failed: {stderr}"))
    }
}

/// Test whether Git is available and can authenticate against a remote.
pub fn probe_git(remote_url: &str) -> Result<ProbeResult> {
    validate_remote_url(remote_url)?;
    let git = TrustedGit::discover()?;
    let version = checked_output(git.run(["--version"], None)?, "Git version check")?;
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    let remote = git.run(["ls-remote", "--heads", remote_url], None)?;
    let auth_ok = remote.status.success();
    Ok(ProbeResult {
        git_version: version,
        auth_ok,
        error: (!auth_ok).then(|| String::from_utf8_lossy(&remote.stderr).trim().to_owned()),
    })
}

/// Clone one source revision into a new destination without submodules or LFS smudging.
pub fn clone(url: &str, dest: &Path, branch: Option<&str>) -> Result<()> {
    validate_remote_url(url)?;
    if let Some(branch) = branch {
        validate_ref(branch)?;
    }
    if dest.exists() {
        return Err(anyhow!("Git clone destination already exists"));
    }
    let git = TrustedGit::discover()?;
    let mut args = vec![
        "clone",
        "--quiet",
        "--progress",
        "--depth",
        "1",
        "--single-branch",
        "--no-tags",
        "--no-recurse-submodules",
    ];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    args.extend(["--", url]);
    let destination = dest
        .to_str()
        .ok_or_else(|| anyhow!("Git clone destination is not UTF-8"))?;
    args.push(destination);
    checked_output(git.run(args, None)?, "Git clone")?;
    Ok(())
}

/// Pull latest changes in a legacy checkout. New acquisition does not use this API.
pub fn pull(repo_dir: &Path) -> Result<PullResult> {
    let git = TrustedGit::discover()?;
    let before = checked_output(
        git.run(["rev-parse", "HEAD"], Some(repo_dir))?,
        "Git revision inspection",
    )?;
    let before_hash = String::from_utf8_lossy(&before.stdout).trim().to_owned();
    let update = git.run(
        ["pull", "--ff-only", "--no-recurse-submodules"],
        Some(repo_dir),
    )?;
    checked_output(update, "Git pull")?;
    let after = checked_output(
        git.run(["rev-parse", "HEAD"], Some(repo_dir))?,
        "Git revision inspection",
    )?;
    let after_hash = String::from_utf8_lossy(&after.stdout).trim().to_owned();
    Ok(PullResult {
        updated: before_hash != after_hash,
        before: before_hash,
        after: after_hash,
    })
}

/// Get the current HEAD commit hash of a repository.
pub fn head_hash(repo_dir: &Path) -> Result<String> {
    let git = TrustedGit::discover()?;
    let output = checked_output(
        git.run(["rev-parse", "--short", "HEAD"], Some(repo_dir))?,
        "Git revision inspection",
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Get the full immutable HEAD commit hash of a repository.
pub fn head_revision(repo_dir: &Path) -> Result<String> {
    let git = TrustedGit::discover()?;
    let output = checked_output(
        git.run(["rev-parse", "HEAD"], Some(repo_dir))?,
        "Git revision inspection",
    )?;
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(anyhow!("Git returned an invalid commit revision"))
    } else {
        Ok(revision)
    }
}

/// Resolve the immutable commit currently selected by a remote source request.
pub fn resolve_remote_revision(remote_url: &str, branch: Option<&str>) -> Result<String> {
    validate_remote_url(remote_url)?;
    if let Some(branch) = branch {
        validate_ref(branch)?;
    }
    let git = TrustedGit::discover()?;
    let reference = branch
        .map(|branch| format!("refs/heads/{branch}"))
        .unwrap_or_else(|| "HEAD".into());
    let output = checked_output(
        git.run(
            [
                "ls-remote".to_owned(),
                "--exit-code".to_owned(),
                remote_url.to_owned(),
                reference.clone(),
            ],
            None,
        )?,
        "Git remote revision inspection",
    )?;
    let mut matches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, remote_ref)| *remote_ref == reference)
        .map(|(revision, _)| revision.to_owned())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    let revision = matches
        .pop()
        .filter(|_| matches.is_empty())
        .ok_or_else(|| anyhow!("Git remote returned an ambiguous revision"))?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(anyhow!("Git remote returned an invalid commit revision"))
    } else {
        Ok(revision)
    }
}

/// Read the configured origin URL without relying on the GUI process PATH.
pub fn remote_url(repo_dir: &Path) -> Result<String> {
    let git = TrustedGit::discover()?;
    let output = checked_output(
        git.run(["remote", "get-url", "origin"], Some(repo_dir))?,
        "Git remote inspection",
    )?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        Err(anyhow!("Git origin URL is empty"))
    } else {
        Ok(value)
    }
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
    use std::os::unix::fs::PermissionsExt;

    use serial_test::serial;

    use super::*;

    #[test]
    fn hostile_remote_and_ref_are_rejected_before_execution() {
        assert!(validate_remote_url("--upload-pack=/tmp/pwn").is_err());
        assert!(validate_remote_url("https://token@example.com/repo.git").is_err());
        assert!(validate_remote_url("https://example.com/repo.git;touch-pwn").is_err());
        assert!(validate_remote_url("git@example.com:../outside.git").is_err());
        assert!(validate_ref("--upload-pack=/tmp/pwn").is_err());
        assert!(validate_ref("main;touch-pwn").is_err());
        assert!(validate_ref("feature/main").is_ok());
    }

    #[test]
    fn nul_environment_parser_preserves_only_complete_entries() {
        let parsed = parse_nul_environment(b"PATH=/usr/bin\0EMPTY=\0broken\0");
        assert_eq!(
            parsed.get(OsStr::new("PATH")),
            Some(&OsString::from("/usr/bin"))
        );
        assert_eq!(parsed.get(OsStr::new("EMPTY")), Some(&OsString::new()));
        assert!(!parsed.contains_key(OsStr::new("broken")));
    }

    #[test]
    fn timed_runner_terminates_the_entire_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("slow-git");
        std::fs::write(&executable, "#!/bin/sh\nsleep 10\n").unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let git = TrustedGit {
            executable,
            environment: BTreeMap::new(),
            policy: GitExecutionPolicy {
                total_timeout: Duration::from_millis(100),
                no_progress_timeout: Duration::from_millis(100),
            },
        };

        let started = Instant::now();
        let error = git.run(["--version"], None).unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(error.to_string().contains("timeout"));
    }

    #[test]
    #[serial(home_env)]
    fn git_version_is_reachable_from_a_gui_style_environment() {
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("PATH", "/usr/bin:/bin");

        let git = TrustedGit::discover().unwrap();
        let output = git.run(["--version"], None).unwrap();

        match previous_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("git version"));
        assert!(git.executable.is_absolute());
    }
}

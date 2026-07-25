// Custom-template launcher.
//
// Users provide a shell command template like:
//   open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}}
//
// We substitute:
//   {{cwd}} -> POSIX-quoted project path
//   {{cmd}} -> POSIX-quoted Agent program, arguments, and environment
//
// The substituted string is then handed to `sh -c`. We intentionally pre-quote
// the substitutions so the template author doesn't have to worry about paths
// containing spaces.

use std::process::Command;

use anyhow::{anyhow, Result};

use super::{shell_quote, LaunchSpec};

pub fn launch(spec: &LaunchSpec<'_>) -> Result<()> {
    let template = spec
        .custom_template
        .ok_or_else(|| anyhow!("custom backend requires a command template"))?;

    let cwd = spec
        .cwd
        .to_str()
        .ok_or_else(|| anyhow!("non-utf8 project path"))?;

    let cmd = render_template(template, cwd, &spec.command());

    let status = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .map_err(|e| anyhow!("failed to spawn shell for custom template: {e}"))?;
    if !status.success() {
        return Err(anyhow!("custom command exited with {status}: {cmd}"));
    }
    Ok(())
}

fn render_template(template: &str, cwd: &str, command: &str) -> String {
    template
        .replace("{{cwd}}", &shell_quote(cwd))
        .replace("{{cmd}}", command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_wezterm_template() {
        let out = render_template(
            "open -na WezTerm.app --args start --cwd {{cwd}} -- {{cmd}}",
            "/Users/me/dev/proj",
            "'claude'",
        );
        assert_eq!(
            out,
            "open -na WezTerm.app --args start --cwd '/Users/me/dev/proj' -- 'claude'"
        );
    }

    #[test]
    fn renders_alacritty_template() {
        let out = render_template(
            "alacritty --working-directory {{cwd}} -e {{cmd}}",
            "/tmp/a b",
            "'claude'",
        );
        assert_eq!(out, "alacritty --working-directory '/tmp/a b' -e 'claude'");
    }

    #[test]
    fn escapes_single_quote_in_path() {
        let out = render_template("foo {{cwd}}", "/tmp/it's", "claude");
        assert!(out.contains("'/tmp/it'\\''s'"), "got: {out}");
    }

    #[test]
    fn missing_placeholders_are_left_alone() {
        // Template with no {{cwd}} is allowed; user may want a static command.
        let out = render_template("echo hello", "/x", "claude");
        assert_eq!(out, "echo hello");
    }

    #[test]
    fn multiple_occurrences_are_replaced() {
        let out = render_template("a {{cwd}} b {{cwd}}", "/p", "c");
        assert_eq!(out, "a '/p' b '/p'");
    }
}

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use super::ResourceKind;

const MAX_DESCRIPTOR_BYTES: u64 = 1024 * 1024;

/// Optional display metadata must not prevent managing an otherwise valid resource.
pub(super) fn read_resource_version(
    source_root: &Path,
    subpath: &str,
    kind: ResourceKind,
) -> Option<String> {
    if Path::new(subpath)
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let source_root = source_root.canonicalize().ok()?;
    let root = source_root.join(subpath).canonicalize().ok()?;
    if !root.starts_with(&source_root) || !root.is_dir() {
        return None;
    }
    match kind {
        ResourceKind::Skills => skill_version(&read_descriptor(&root, "SKILL.md")?),
        ResourceKind::Plugins => {
            let mut versions = std::collections::BTreeSet::new();
            for descriptor in [".claude-plugin/plugin.json", ".codex-plugin/plugin.json"] {
                let Some(text) = read_descriptor(&root, descriptor) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                if let Some(version) = value.get("version").and_then(serde_json::Value::as_str) {
                    if let Some(version) = display_version(version) {
                        versions.insert(version);
                    }
                }
            }
            // A shared card must not claim one version when Agent manifests disagree.
            (versions.len() == 1)
                .then(|| versions.into_iter().next())
                .flatten()
        }
        _ => None,
    }
}

fn read_descriptor(root: &Path, relative: &str) -> Option<String> {
    let path = root.join(relative).canonicalize().ok()?;
    if !path.starts_with(root) {
        return None;
    }
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DESCRIPTOR_BYTES {
        return None;
    }
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_DESCRIPTOR_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_DESCRIPTOR_BYTES).then_some(text)
}

fn skill_version(text: &str) -> Option<String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines {
        if matches!(line.trim_end(), "---" | "...") {
            closed = true;
            break;
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    if !closed {
        return None;
    }
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&frontmatter).ok()?;
    let version = value
        .get("metadata")
        .and_then(|metadata| metadata.get("version"))
        .or_else(|| value.get("version"))?;
    display_version(version.as_str()?)
}

fn display_version(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_skill_metadata_without_mistaking_other_yaml_or_body_for_version() {
        for (text, expected) in [
            (
                "---\nname: review\nmetadata:\n  version: '1.8.5'\n---\n",
                Some("1.8.5"),
            ),
            (
                "---\nmetadata: {version: \"2.0-beta\"}\n---\n",
                Some("2.0-beta"),
            ),
            (
                "\u{feff}---\r\nversion: '1.0' # release\r\n---\r\n",
                Some("1.0"),
            ),
            (
                "---\nversion: old\nmetadata:\n  version: new\n---\n",
                Some("new"),
            ),
            ("---\nname: review\n---\nversion: 9.9.9\n", None),
            ("---\ndescription: |\n  version: 9.9.9\n---\n", None),
            (
                "---\ndescription: |\n  ---\n  Example\nmetadata: {version: '1.0.0'}\n---\n",
                Some("1.0.0"),
            ),
            ("---\nother:\n  version: 9.9.9\n---\n", None),
            ("---\nmetadata:\n  version: ''\n---\n", None),
            ("---\nversion: null\n---\n", None),
            ("---\nversion: [1, 2, 3]\n---\n", None),
            ("---\nversion: [invalid\n---\n", None),
            ("---\nversion: 1.2.3\n", None),
        ] {
            assert_eq!(skill_version(text).as_deref(), expected, "{text}");
        }
    }

    #[test]
    fn refresh_reads_changed_and_removed_versions_without_inheriting_parent_metadata() {
        let source = tempfile::tempdir().unwrap();
        let skill = source.path().join("review");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(source.path().join("package.json"), r#"{"version":"9.0.0"}"#).unwrap();
        for (declaration, expected) in [
            ("metadata: {version: '1.0.0'}", Some("1.0.0")),
            ("metadata: {version: '2.0.0'}", Some("2.0.0")),
            ("name: review", None),
        ] {
            std::fs::write(skill.join("SKILL.md"), format!("---\n{declaration}\n---\n")).unwrap();
            assert_eq!(
                read_resource_version(source.path(), "review", ResourceKind::Skills).as_deref(),
                expected
            );
        }
    }

    #[test]
    fn native_plugin_versions_must_agree_and_never_come_from_marketplace() {
        let source = tempfile::tempdir().unwrap();
        for directory in [".claude-plugin", ".codex-plugin"] {
            std::fs::create_dir(source.path().join(directory)).unwrap();
        }
        let claude = source.path().join(".claude-plugin/plugin.json");
        let codex = source.path().join(".codex-plugin/plugin.json");
        std::fs::write(&claude, r#"{"name":"tool","version":"1.8.5"}"#).unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Plugins).as_deref(),
            Some("1.8.5")
        );
        std::fs::write(&codex, r#"{"name":"tool","version":"1.8.5"}"#).unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Plugins).as_deref(),
            Some("1.8.5")
        );
        std::fs::write(&codex, r#"{"name":"tool","version":"2.0.0"}"#).unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Plugins),
            None
        );
        std::fs::remove_file(&claude).unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Plugins).as_deref(),
            Some("2.0.0")
        );
        for invalid in [
            r#"{"version":" "}"#,
            r#"{"version":123}"#,
            "{invalid",
            r#"{"name":"tool"}"#,
        ] {
            std::fs::write(&codex, invalid).unwrap();
            std::fs::write(
                source.path().join(".claude-plugin/marketplace.json"),
                r#"{"version":"9.0.0"}"#,
            )
            .unwrap();
            assert_eq!(
                read_resource_version(source.path(), "", ResourceKind::Plugins),
                None
            );
        }
    }

    #[test]
    fn ignores_escaping_paths_symlinks_and_oversized_descriptors() {
        let source = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(
            external.path().join("SKILL.md"),
            "---\nversion: secret\n---\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(external.path(), source.path().join("escape")).unwrap();
        assert_eq!(
            read_resource_version(source.path(), "escape", ResourceKind::Skills),
            None
        );
        assert_eq!(
            read_resource_version(source.path(), "../", ResourceKind::Skills),
            None
        );
        std::os::unix::fs::symlink(
            external.path().join("SKILL.md"),
            source.path().join("SKILL.md"),
        )
        .unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Skills),
            None
        );
        std::fs::remove_file(source.path().join("SKILL.md")).unwrap();
        std::fs::write(
            source.path().join("SKILL.md"),
            "x".repeat(MAX_DESCRIPTOR_BYTES as usize + 1),
        )
        .unwrap();
        assert_eq!(
            read_resource_version(source.path(), "", ResourceKind::Skills),
            None
        );
    }
}

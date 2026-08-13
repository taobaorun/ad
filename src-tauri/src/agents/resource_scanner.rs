use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::skill_activation::inspect_skills;
use super::skill_artifact_tree::{inspect_tree, ArtifactLimits, TreeEntryKind, TreeManifest};
use super::{ContentDigest, ResourceKind, SkillArtifactError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScannedCatalogResource {
    pub kind: ResourceKind,
    pub install_id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub subpath: String,
    pub descriptor_digest: ContentDigest,
    pub supported_agents: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct PluginCandidate {
    install_id: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    descriptor_digests: Vec<ContentDigest>,
    supported_agents: BTreeSet<String>,
}

pub(crate) fn scan_catalog_resources(
    physical_root: &Path,
) -> Result<Vec<ScannedCatalogResource>, SkillArtifactError> {
    let manifest = inspect_tree(physical_root, ArtifactLimits::default())?;
    scan_manifest_resources(physical_root, &manifest)
}

fn scan_manifest_resources(
    physical_root: &Path,
    manifest: &TreeManifest,
) -> Result<Vec<ScannedCatalogResource>, SkillArtifactError> {
    let mut plugin_roots = BTreeMap::<String, PluginCandidate>::new();
    for entry in &manifest.entries {
        if entry.kind != TreeEntryKind::File || !is_native_plugin_descriptor(&entry.path) {
            continue;
        }
        let descriptor_path = Path::new(&entry.path);
        let plugin_root = descriptor_path
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new(""));
        let subpath = normalized_subpath(plugin_root)?;
        let bytes = std::fs::read(physical_root.join(descriptor_path)).map_err(|source| {
            SkillArtifactError::Io {
                path: physical_root.join(descriptor_path).display().to_string(),
                source,
            }
        })?;
        let descriptor: Value = serde_json::from_slice(&bytes).map_err(|error| {
            SkillArtifactError::InvalidSource(format!(
                "Plugin descriptor {} is invalid JSON: {error}",
                entry.path
            ))
        })?;
        let object = descriptor.as_object().ok_or_else(|| {
            SkillArtifactError::InvalidSource(format!(
                "Plugin descriptor {} must be a JSON object",
                entry.path
            ))
        })?;
        let install_id = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| valid_install_id(value))
            .ok_or_else(|| {
                SkillArtifactError::InvalidSource(format!(
                    "Plugin descriptor {} has no valid name",
                    entry.path
                ))
            })?;
        let candidate = plugin_roots.entry(subpath).or_default();
        if candidate
            .install_id
            .as_deref()
            .is_some_and(|current| current != install_id)
        {
            return Err(SkillArtifactError::InvalidSource(format!(
                "native Plugin descriptors at one resource root disagree on install ID: {}",
                entry.path
            )));
        }
        candidate.install_id = Some(install_id.to_owned());
        candidate.display_name = object
            .get("displayName")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| Some(install_id.to_owned()));
        candidate.description = object
            .get("description")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        candidate.descriptor_digests.push(
            entry.content_digest.clone().ok_or_else(|| {
                SkillArtifactError::Corrupt(format!("{} has no digest", entry.path))
            })?,
        );
        let agent_id = match descriptor_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
        {
            Some(".claude-plugin") => "claude-code",
            Some(".codex-plugin") => "codex",
            _ => unreachable!("native descriptor path was validated"),
        };
        candidate.supported_agents.insert(agent_id.into());
    }

    let plugin_paths = plugin_roots.keys().map(PathBuf::from).collect::<Vec<_>>();
    let mut resources = Vec::new();
    for (subpath, candidate) in plugin_roots {
        let install_id = candidate
            .install_id
            .expect("validated Plugin candidate has an install ID");
        let digest_input = candidate
            .descriptor_digests
            .iter()
            .map(ContentDigest::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        resources.push(ScannedCatalogResource {
            kind: ResourceKind::Plugins,
            install_id: install_id.clone(),
            display_name: candidate.display_name.unwrap_or(install_id),
            description: candidate.description,
            subpath,
            descriptor_digest: ContentDigest::sha256(digest_input.as_bytes()),
            supported_agents: candidate.supported_agents,
        });
    }

    for skill in inspect_skills(physical_root, manifest)? {
        let skill_path = Path::new(&skill.subpath);
        if plugin_paths
            .iter()
            .any(|plugin| skill_path == plugin || skill_path.starts_with(plugin))
        {
            continue;
        }
        resources.push(ScannedCatalogResource {
            kind: ResourceKind::Skills,
            install_id: skill.logical_id.clone(),
            display_name: skill.logical_id,
            description: None,
            subpath: skill.subpath,
            descriptor_digest: skill.instruction_digest,
            supported_agents: BTreeSet::from(["claude-code".into(), "codex".into()]),
        });
    }
    resources.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.install_id.cmp(&right.install_id))
            .then_with(|| left.subpath.cmp(&right.subpath))
    });
    let mut identities = BTreeSet::new();
    for resource in &resources {
        if !identities.insert((resource.kind, resource.install_id.to_ascii_lowercase())) {
            return Err(SkillArtifactError::InvalidSource(format!(
                "source contains duplicate {:?} install ID: {}",
                resource.kind, resource.install_id
            )));
        }
    }
    if resources.is_empty() {
        return Err(SkillArtifactError::InvalidSource(
            "source contains no standard Skill or Plugin resources".into(),
        ));
    }
    Ok(resources)
}

fn is_native_plugin_descriptor(path: &str) -> bool {
    let path = Path::new(path);
    path.file_name().and_then(|value| value.to_str()) == Some("plugin.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .is_some_and(|value| matches!(value, ".claude-plugin" | ".codex-plugin"))
}

fn normalized_subpath(path: &Path) -> Result<String, SkillArtifactError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(SkillArtifactError::InvalidSource(
            "resource root is not a safe relative path".into(),
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

fn valid_install_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.trim() == value
        && !value.chars().any(char::is_control)
        && !value.contains('/')
        && value != "."
        && value != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_standalone_skills_and_native_plugins_without_splitting_plugin_skills() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("review")).unwrap();
        std::fs::write(
            source.path().join("review/SKILL.md"),
            "---\nname: review\n---\n",
        )
        .unwrap();
        std::fs::create_dir_all(source.path().join("tool/.claude-plugin")).unwrap();
        std::fs::create_dir_all(source.path().join("tool/skills/internal")).unwrap();
        std::fs::write(
            source.path().join("tool/.claude-plugin/plugin.json"),
            r#"{"name":"tool","description":"Native tool"}"#,
        )
        .unwrap();
        std::fs::write(
            source.path().join("tool/skills/internal/SKILL.md"),
            "---\nname: internal\n---\n",
        )
        .unwrap();

        let resources = scan_catalog_resources(source.path()).unwrap();

        assert_eq!(resources.len(), 2);
        assert!(resources.iter().any(|resource| {
            resource.kind == ResourceKind::Skills && resource.install_id == "review"
        }));
        assert!(resources.iter().any(|resource| {
            resource.kind == ResourceKind::Plugins
                && resource.install_id == "tool"
                && resource.subpath == "tool"
        }));
        assert!(!resources
            .iter()
            .any(|resource| resource.install_id == "internal"));
    }

    #[test]
    fn rejects_duplicate_install_ids_inside_one_source() {
        let source = tempfile::tempdir().unwrap();
        for directory in ["one", "two"] {
            std::fs::create_dir_all(source.path().join(directory)).unwrap();
            std::fs::write(
                source.path().join(directory).join("SKILL.md"),
                "---\nname: duplicate\n---\n",
            )
            .unwrap();
        }

        let error = scan_catalog_resources(source.path()).unwrap_err();

        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_mismatched_native_descriptors_for_one_plugin_root() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".claude-plugin")).unwrap();
        std::fs::create_dir_all(source.path().join(".codex-plugin")).unwrap();
        std::fs::write(
            source.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"claude-name"}"#,
        )
        .unwrap();
        std::fs::write(
            source.path().join(".codex-plugin/plugin.json"),
            r#"{"name":"codex-name"}"#,
        )
        .unwrap();

        let error = scan_catalog_resources(source.path()).unwrap_err();

        assert!(error.to_string().contains("disagree"));
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::skill_activation::inspect_skills;
use super::skill_artifact_tree::{
    inspect_tree_filtered, ArtifactLimits, ArtifactTreeError, TreeEntryKind, TreeManifest,
};
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
    let manifest = inspect_catalog_tree(physical_root)?;
    scan_manifest_resources(physical_root, &manifest)
}

pub(super) fn inspect_catalog_tree(root: &Path) -> Result<TreeManifest, ArtifactTreeError> {
    inspect_tree_filtered(root, ArtifactLimits::default(), &|path| {
        !is_agent_skill_projection(path)
    })
}

pub(super) fn is_agent_skill_projection(path: &Path) -> bool {
    let mut previous_is_agent_root = false;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            previous_is_agent_root = false;
            continue;
        };
        if previous_is_agent_root && name == "skills" {
            return true;
        }
        previous_is_agent_root = matches!(name.to_str(), Some(".agents" | ".claude" | ".codex"));
    }
    false
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
    for (subpath, mut candidate) in plugin_roots {
        let install_id = candidate
            .install_id
            .expect("validated Plugin candidate has an install ID");
        if candidate.supported_agents.contains("codex")
            && super::codex_plugins::read_codex_catalog_plugin_metadata(
                &physical_root.join(&subpath),
                &install_id,
            )
            .is_err()
        {
            candidate.supported_agents.remove("codex");
        }
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
    let preferred_roots = root_marketplace_local_plugin_roots(physical_root, manifest);
    let preferred_ids = resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Plugins)
        .filter_map(|resource| {
            let install_id = resource.install_id.to_ascii_lowercase();
            preferred_roots
                .get(&install_id)
                .is_some_and(|subpath| subpath == &resource.subpath)
                .then_some(install_id)
        })
        .collect::<BTreeSet<_>>();
    resources.retain(|resource| {
        resource.kind != ResourceKind::Plugins
            || !resource.subpath.is_empty()
            || !preferred_ids.contains(&resource.install_id.to_ascii_lowercase())
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

fn root_marketplace_local_plugin_roots(
    physical_root: &Path,
    manifest: &TreeManifest,
) -> BTreeMap<String, String> {
    const MARKETPLACE_PATH: &str = ".claude-plugin/marketplace.json";
    if !manifest
        .entries
        .iter()
        .any(|entry| entry.kind == TreeEntryKind::File && entry.path == MARKETPLACE_PATH)
    {
        return BTreeMap::new();
    }
    let Ok(bytes) = std::fs::read(physical_root.join(MARKETPLACE_PATH)) else {
        return BTreeMap::new();
    };
    let Ok(marketplace) = serde_json::from_slice::<Value>(&bytes) else {
        return BTreeMap::new();
    };
    let Some(plugins) = marketplace.get("plugins").and_then(Value::as_array) else {
        return BTreeMap::new();
    };
    let mut roots = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for plugin in plugins {
        let Some(install_id) = plugin
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| valid_install_id(value))
        else {
            continue;
        };
        let Some(subpath) = plugin
            .get("source")
            .and_then(marketplace_local_source_subpath)
        else {
            continue;
        };
        let key = install_id.to_ascii_lowercase();
        if roots.get(&key).is_some_and(|current| current != &subpath) {
            ambiguous.insert(key);
        } else {
            roots.insert(key, subpath);
        }
    }
    for install_id in ambiguous {
        roots.remove(&install_id);
    }
    roots
}

fn marketplace_local_source_subpath(source: &Value) -> Option<String> {
    let value = match source {
        Value::String(value) => value.as_str(),
        Value::Object(value) if value.get("source").and_then(Value::as_str) == Some("local") => {
            value.get("path")?.as_str()?
        }
        _ => return None,
    };
    let path = Path::new(value);
    if path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(value) => normalized.push(value),
            _ => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then(|| normalized.to_string_lossy().into_owned())
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
    fn prefers_marketplace_declared_package_over_duplicate_root_plugin() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".claude-plugin")).unwrap();
        std::fs::create_dir_all(source.path().join("plugin/.claude-plugin")).unwrap();
        for descriptor in [
            source.path().join(".claude-plugin/plugin.json"),
            source.path().join("plugin/.claude-plugin/plugin.json"),
        ] {
            std::fs::write(descriptor, r#"{"name":"impeccable"}"#).unwrap();
        }
        std::fs::write(
            source.path().join(".claude-plugin/marketplace.json"),
            r#"{"name":"impeccable","plugins":[{"name":"impeccable","source":"./plugin"}]}"#,
        )
        .unwrap();

        let resources = scan_catalog_resources(source.path()).unwrap();

        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, ResourceKind::Plugins);
        assert_eq!(resources[0].install_id, "impeccable");
        assert_eq!(resources[0].subpath, "plugin");
    }

    #[test]
    fn escaping_marketplace_source_does_not_suppress_duplicate_plugin() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join(".claude-plugin")).unwrap();
        std::fs::create_dir_all(source.path().join("plugin/.claude-plugin")).unwrap();
        for descriptor in [
            source.path().join(".claude-plugin/plugin.json"),
            source.path().join("plugin/.claude-plugin/plugin.json"),
        ] {
            std::fs::write(descriptor, r#"{"name":"impeccable"}"#).unwrap();
        }
        std::fs::write(
            source.path().join(".claude-plugin/marketplace.json"),
            r#"{"plugins":[{"name":"impeccable","source":"../plugin"}]}"#,
        )
        .unwrap();

        let error = scan_catalog_resources(source.path()).unwrap_err();

        assert!(error
            .to_string()
            .contains("duplicate Plugins install ID: impeccable"));
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

    #[test]
    fn codex_plugin_requires_a_root_local_marketplace_declaration() {
        let source = tempfile::tempdir().unwrap();
        let plugin = source.path().join("tool");
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(plugin.join(".agents/plugins")).unwrap();
        std::fs::write(
            plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"tool","version":"1.2.3"}"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join(".agents/plugins/marketplace.json"),
            r#"{"name":"team","plugins":[{"name":"tool","source":{"source":"local","path":"./"}}]}"#,
        )
        .unwrap();

        let resources = scan_catalog_resources(source.path()).unwrap();
        let tool = resources
            .iter()
            .find(|resource| resource.install_id == "tool")
            .unwrap();
        assert!(tool.supported_agents.contains("codex"));

        std::fs::remove_file(plugin.join(".agents/plugins/marketplace.json")).unwrap();
        let resources = scan_catalog_resources(source.path()).unwrap();
        let tool = resources
            .iter()
            .find(|resource| resource.install_id == "tool")
            .unwrap();
        assert!(!tool.supported_agents.contains("codex"));
    }

    #[test]
    fn ignores_agent_skill_projections_but_rejects_absolute_links_elsewhere() {
        let source = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let plugin = source.path().join("tool");
        std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(plugin.join(".agents/plugins")).unwrap();
        std::fs::create_dir_all(plugin.join(".agents/skills")).unwrap();
        std::fs::write(
            plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"tool","version":"1.2.3"}"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join(".agents/plugins/marketplace.json"),
            r#"{"name":"team","plugins":[{"name":"tool","source":{"source":"local","path":"./"}}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(external.path().join("external-skill")).unwrap();
        std::fs::write(
            external.path().join("external-skill/SKILL.md"),
            "---\nname: external-skill\n---\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            external.path().join("external-skill"),
            plugin.join(".agents/skills/external-skill"),
        )
        .unwrap();

        let resources = scan_catalog_resources(source.path()).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].install_id, "tool");

        std::os::unix::fs::symlink(
            external.path().join("external-skill"),
            plugin.join("unsafe-link"),
        )
        .unwrap();
        let error = scan_catalog_resources(source.path()).unwrap_err();
        assert!(error.to_string().contains("absolute symlink"));
    }
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fs::atomic::write_atomic;
use crate::fs::paths::ad_home;

use super::execution_fs::write_directory_atomic;
use super::{directory_tree_digest, ContentDigest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeMarketplaceDescriptor {
    pub name: String,
    pub source_type: String,
    pub source: String,
    pub materialized_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginComponents {
    #[serde(default)]
    pub skills: Vec<PathBuf>,
    #[serde(default)]
    pub commands: Vec<PathBuf>,
    #[serde(default)]
    pub hooks: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Value>,
    #[serde(default)]
    pub apps: Vec<PathBuf>,
    #[serde(default)]
    pub agents: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsp_servers: Option<Value>,
    #[serde(default)]
    pub unknown: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_manifest: Option<PathBuf>,
}

impl PluginComponents {
    pub fn portable_count(&self) -> usize {
        self.skills.len() + self.commands.len() + usize::from(self.mcp_servers.is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginDescriptor {
    pub plugin_id: String,
    pub enabled: bool,
    pub declaration_path: PathBuf,
    pub marketplace: ClaudeMarketplaceDescriptor,
    pub source_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub components: PluginComponents,
    pub source_digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudePluginRoute {
    SourceDisabled,
    PackageCopy,
    PackageTransform,
    ComponentFallback,
    Partial,
    UnsupportedComponent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudePluginClassification {
    pub route: ClaudePluginRoute,
    pub portable_component_count: usize,
    #[serde(default)]
    pub residual_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedProjectPluginInstall {
    pub logical_id: String,
    pub source: Value,
    pub marketplace_digest: ContentDigest,
    pub package_digest: ContentDigest,
}

pub fn classify_claude_plugin(
    descriptor: Option<&ClaudePluginDescriptor>,
    enabled: bool,
) -> ClaudePluginClassification {
    if !enabled {
        return ClaudePluginClassification {
            route: ClaudePluginRoute::SourceDisabled,
            portable_component_count: 0,
            residual_reasons: Vec::new(),
        };
    }
    let Some(descriptor) = descriptor else {
        return ClaudePluginClassification {
            route: ClaudePluginRoute::UnsupportedComponent,
            portable_component_count: 0,
            residual_reasons: vec!["Claude Plugin package could not be resolved".into()],
        };
    };
    let components = &descriptor.components;
    if components.codex_manifest.is_some() {
        return ClaudePluginClassification {
            route: ClaudePluginRoute::PackageCopy,
            portable_component_count: components.portable_count(),
            residual_reasons: residual_reasons(components),
        };
    }

    let portable_component_count = components.portable_count();
    let residual_reasons = residual_reasons(components);
    let route = if residual_reasons.is_empty() && portable_component_count > 0 {
        ClaudePluginRoute::PackageTransform
    } else if portable_component_count > 0 {
        ClaudePluginRoute::Partial
    } else {
        ClaudePluginRoute::UnsupportedComponent
    };
    ClaudePluginClassification {
        route,
        portable_component_count,
        residual_reasons,
    }
}

pub fn inspect_claude_plugin(
    claude_home: &Path,
    project_path: &Path,
    plugin_id: &str,
    enabled: bool,
) -> Result<ClaudePluginDescriptor, ClaudePluginInspectionError> {
    let (plugin_name, marketplace_name) = split_plugin_id(plugin_id)?;
    let canonical_project =
        std::fs::canonicalize(project_path).map_err(|source| ClaudePluginInspectionError::Io {
            path: project_path.to_path_buf(),
            source,
        })?;
    let installed_path = claude_home.join("plugins/installed_plugins.json");
    let installed: InstalledRegistry = read_json(&installed_path)?;
    let entries = installed
        .plugins
        .get(plugin_id)
        .ok_or_else(|| ClaudePluginInspectionError::PluginNotInstalled(plugin_id.to_string()))?;
    let entry = entries
        .iter()
        .find(|entry| {
            matches!(entry.scope.as_str(), "local" | "project")
                && entry
                    .project_path
                    .as_deref()
                    .and_then(|path| std::fs::canonicalize(path).ok())
                    .as_deref()
                    == Some(canonical_project.as_path())
        })
        .or_else(|| entries.iter().find(|entry| entry.scope == "user"))
        .ok_or_else(|| ClaudePluginInspectionError::NoProjectPackage(plugin_id.to_string()))?;
    let source_root = std::fs::canonicalize(&entry.install_path).map_err(|source| {
        ClaudePluginInspectionError::Io {
            path: entry.install_path.clone(),
            source,
        }
    })?;
    if !source_root.is_dir() {
        return Err(ClaudePluginInspectionError::InvalidPackage(
            source_root.to_string_lossy().into_owned(),
        ));
    }
    let source_digest =
        directory_tree_digest(&source_root).map_err(|source| ClaudePluginInspectionError::Io {
            path: source_root.clone(),
            source,
        })?;
    let marketplace = inspect_marketplace(claude_home, marketplace_name)?;
    let components = inspect_components(&source_root, plugin_name, entry.version.as_deref())?;

    Ok(ClaudePluginDescriptor {
        plugin_id: plugin_id.to_string(),
        enabled,
        declaration_path: installed_path,
        marketplace,
        source_root,
        version: entry.version.clone(),
        components,
        source_digest,
    })
}

pub fn prepare_project_plugin_install(
    descriptor: &ClaudePluginDescriptor,
) -> Result<PreparedProjectPluginInstall, ClaudePluginInspectionError> {
    let classification = classify_claude_plugin(Some(descriptor), descriptor.enabled);
    if !matches!(
        classification.route,
        ClaudePluginRoute::PackageCopy
            | ClaudePluginRoute::PackageTransform
            | ClaudePluginRoute::Partial
    ) {
        return Err(ClaudePluginInspectionError::UnsupportedRoute(
            classification.route,
        ));
    }
    let version = descriptor
        .version
        .as_deref()
        .filter(|version| !version.is_empty() && *version != "unknown")
        .unwrap_or("local")
        .to_string();
    let marketplace_source_digest =
        directory_tree_digest(&descriptor.marketplace.materialized_path).map_err(|source| {
            ClaudePluginInspectionError::Io {
                path: descriptor.marketplace.materialized_path.clone(),
                source,
            }
        })?;
    let staging_identity = serde_json::to_vec(&serde_json::json!({
        "pluginId": descriptor.plugin_id,
        "version": version,
        "sourceDigest": descriptor.source_digest,
        "marketplace": {
            "name": descriptor.marketplace.name,
            "sourceType": descriptor.marketplace.source_type,
            "source": descriptor.marketplace.source,
            "refName": descriptor.marketplace.ref_name,
            "lastRevision": descriptor.marketplace.last_revision,
            "sourceDigest": marketplace_source_digest,
        }
    }))
    .map_err(|error| ClaudePluginInspectionError::Stage(error.to_string()))?;
    let staging_digest = ContentDigest::sha256(&staging_identity);
    let digest_key = staging_digest
        .as_str()
        .strip_prefix("sha256:")
        .unwrap_or(staging_digest.as_str());
    let root = ad_home()
        .map_err(|error| ClaudePluginInspectionError::Stage(error.to_string()))?
        .join("staging/codex-plugin-conversion")
        .join(digest_key);
    std::fs::create_dir_all(&root).map_err(|source| ClaudePluginInspectionError::Io {
        path: root.clone(),
        source,
    })?;
    let marketplace_stage = root.join("marketplace");
    materialize_stage_directory(
        &descriptor.marketplace.materialized_path,
        &marketplace_stage,
    )?;
    let package_stage = root.join("package");
    materialize_stage_directory(&descriptor.source_root, &package_stage)?;
    if matches!(
        classification.route,
        ClaudePluginRoute::PackageTransform | ClaudePluginRoute::Partial
    ) {
        transform_commands_to_skills(&package_stage, &descriptor.components.commands)?;
        write_generated_codex_manifest(&package_stage, descriptor, &version)?;
    }
    let marketplace_digest = directory_tree_digest(&marketplace_stage).map_err(|source| {
        ClaudePluginInspectionError::Io {
            path: marketplace_stage.clone(),
            source,
        }
    })?;
    let package_digest = directory_tree_digest(&package_stage).map_err(|source| {
        ClaudePluginInspectionError::Io {
            path: package_stage.clone(),
            source,
        }
    })?;
    let (plugin_name, _) = split_plugin_id(&descriptor.plugin_id)?;
    Ok(PreparedProjectPluginInstall {
        logical_id: descriptor.plugin_id.clone(),
        source: serde_json::json!({
            "marketplace": {
                "name": descriptor.marketplace.name,
                "sourceType": descriptor.marketplace.source_type,
                "source": descriptor.marketplace.source,
                "refName": descriptor.marketplace.ref_name,
                "lastRevision": descriptor.marketplace.last_revision,
                "stagePath": marketplace_stage,
            },
            "package": {
                "name": plugin_name,
                "version": version,
                "stagePath": package_stage,
            }
        }),
        marketplace_digest,
        package_digest,
    })
}

fn materialize_stage_directory(
    source: &Path,
    target: &Path,
) -> Result<(), ClaudePluginInspectionError> {
    let source_digest =
        directory_tree_digest(source).map_err(|source_error| ClaudePluginInspectionError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    if target.is_dir()
        && directory_tree_digest(target)
            .map(|digest| digest == source_digest)
            .unwrap_or(false)
    {
        return Ok(());
    }
    write_directory_atomic(target, source).map_err(|source| ClaudePluginInspectionError::Io {
        path: target.to_path_buf(),
        source,
    })
}

fn transform_commands_to_skills(
    package_stage: &Path,
    source_commands: &[PathBuf],
) -> Result<(), ClaudePluginInspectionError> {
    for source in source_commands {
        if !source.is_file()
            || source.extension().and_then(|extension| extension.to_str()) != Some("md")
        {
            continue;
        }
        let name = source
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| valid_segment(name))
            .ok_or_else(|| {
                ClaudePluginInspectionError::InvalidPackage(source.display().to_string())
            })?;
        let file_name = source.file_name().ok_or_else(|| {
            ClaudePluginInspectionError::InvalidPackage(source.display().to_string())
        })?;
        let staged_source = package_stage.join("commands").join(file_name);
        let bytes =
            std::fs::read(&staged_source).map_err(|source| ClaudePluginInspectionError::Io {
                path: staged_source.clone(),
                source,
            })?;
        let target = package_stage.join("skills").join(name).join("SKILL.md");
        write_atomic(&target, &bytes)
            .map_err(|error| ClaudePluginInspectionError::Stage(error.to_string()))?;
    }
    Ok(())
}

fn write_generated_codex_manifest(
    package_stage: &Path,
    descriptor: &ClaudePluginDescriptor,
    version: &str,
) -> Result<(), ClaudePluginInspectionError> {
    let (name, _) = split_plugin_id(&descriptor.plugin_id)?;
    let mut manifest = serde_json::json!({
        "name": name,
        "version": version,
        "description": "Converted by AD from a local Claude Code Plugin package"
    });
    let object = manifest
        .as_object_mut()
        .expect("generated manifest is an object");
    if package_stage.join("skills").is_dir() {
        object.insert("skills".into(), Value::String("./skills/".into()));
    }
    if package_stage.join(".mcp.json").is_file() {
        object.insert("mcpServers".into(), Value::String("./.mcp.json".into()));
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| ClaudePluginInspectionError::Stage(error.to_string()))?;
    bytes.push(b'\n');
    write_atomic(&package_stage.join(".codex-plugin/plugin.json"), &bytes)
        .map_err(|error| ClaudePluginInspectionError::Stage(error.to_string()))
}

fn inspect_marketplace(
    claude_home: &Path,
    name: &str,
) -> Result<ClaudeMarketplaceDescriptor, ClaudePluginInspectionError> {
    let path = claude_home.join("plugins/known_marketplaces.json");
    let known: BTreeMap<String, KnownMarketplace> = read_json(&path)?;
    let marketplace = known
        .get(name)
        .ok_or_else(|| ClaudePluginInspectionError::MarketplaceNotFound(name.to_string()))?;
    let materialized_path =
        std::fs::canonicalize(&marketplace.install_location).map_err(|source| {
            ClaudePluginInspectionError::Io {
                path: marketplace.install_location.clone(),
                source,
            }
        })?;
    let (source_type, source) =
        match marketplace.source.kind.as_str() {
            "github" => {
                let repo = marketplace.source.repo.as_deref().ok_or_else(|| {
                    ClaudePluginInspectionError::InvalidMarketplace(name.to_string())
                })?;
                ("git".to_string(), format!("https://github.com/{repo}.git"))
            }
            "git" => {
                let url = marketplace.source.url.as_deref().ok_or_else(|| {
                    ClaudePluginInspectionError::InvalidMarketplace(name.to_string())
                })?;
                ("git".to_string(), url.to_string())
            }
            "directory" => {
                let path = marketplace.source.path.as_deref().ok_or_else(|| {
                    ClaudePluginInspectionError::InvalidMarketplace(name.to_string())
                })?;
                let source = std::fs::canonicalize(path).map_err(|source| {
                    ClaudePluginInspectionError::Io {
                        path: path.to_path_buf(),
                        source,
                    }
                })?;
                ("local".to_string(), source.to_string_lossy().into_owned())
            }
            _ => {
                return Err(ClaudePluginInspectionError::UnsupportedMarketplace(
                    name.to_string(),
                ))
            }
        };
    Ok(ClaudeMarketplaceDescriptor {
        name: name.to_string(),
        source_type,
        source,
        materialized_path,
        ref_name: None,
        // A Plugin install SHA is not authoritative for the shared marketplace checkout.
        last_revision: None,
    })
}

fn inspect_components(
    source_root: &Path,
    expected_name: &str,
    expected_version: Option<&str>,
) -> Result<PluginComponents, ClaudePluginInspectionError> {
    let codex_manifest = source_root.join(".codex-plugin/plugin.json");
    let claude_manifest = [
        source_root.join(".claude-plugin/plugin.json"),
        source_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file());
    let manifest_path = if codex_manifest.is_file() {
        Some(codex_manifest.clone())
    } else {
        claude_manifest
    };
    let manifest = manifest_path
        .as_deref()
        .map(read_json::<Value>)
        .transpose()?
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    validate_manifest_identity(&manifest, expected_name, expected_version)?;

    let mut components = PluginComponents {
        skills: child_entries(&source_root.join("skills"))?,
        commands: child_entries(&source_root.join("commands"))?,
        hooks: hook_entries(source_root)?,
        mcp_servers: read_optional_json(&source_root.join(".mcp.json"))?
            .or_else(|| manifest.get("mcpServers").cloned()),
        apps: child_entries(&source_root.join("apps"))?,
        agents: child_entries(&source_root.join("agents"))?,
        lsp_servers: read_optional_json(&source_root.join(".lsp.json"))?
            .or_else(|| manifest.get("lspServers").cloned()),
        unknown: Vec::new(),
        codex_manifest: codex_manifest.is_file().then_some(codex_manifest),
    };
    if let Some(object) = manifest.as_object() {
        let known = [
            "name",
            "version",
            "description",
            "author",
            "homepage",
            "repository",
            "license",
            "keywords",
            "skills",
            "commands",
            "hooks",
            "mcpServers",
            "apps",
            "agents",
            "lspServers",
        ];
        components.unknown = object
            .keys()
            .filter(|key| !known.contains(&key.as_str()))
            .cloned()
            .collect();
    }
    Ok(components)
}

fn validate_manifest_identity(
    manifest: &Value,
    expected_name: &str,
    expected_version: Option<&str>,
) -> Result<(), ClaudePluginInspectionError> {
    if let Some(name) = manifest.get("name").and_then(Value::as_str) {
        if name != expected_name {
            return Err(ClaudePluginInspectionError::ManifestIdentityMismatch);
        }
    }
    if let (Some(expected), Some(version)) = (
        expected_version.filter(|version| *version != "unknown"),
        manifest.get("version").and_then(Value::as_str),
    ) {
        if version != expected {
            return Err(ClaudePluginInspectionError::ManifestIdentityMismatch);
        }
    }
    Ok(())
}

fn child_entries(directory: &Path) -> Result<Vec<PathBuf>, ClaudePluginInspectionError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ClaudePluginInspectionError::Io {
                path: directory.to_path_buf(),
                source,
            })
        }
    };
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| ClaudePluginInspectionError::Io {
                    path: directory.to_path_buf(),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn hook_entries(source_root: &Path) -> Result<Vec<PathBuf>, ClaudePluginInspectionError> {
    let mut hooks = child_entries(&source_root.join("hooks"))?;
    for candidate in [
        source_root.join("hooks.json"),
        source_root.join(".hooks.json"),
    ] {
        if candidate.is_file() {
            hooks.push(candidate);
        }
    }
    hooks.sort();
    Ok(hooks)
}

fn read_optional_json(path: &Path) -> Result<Option<Value>, ClaudePluginInspectionError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|source| {
            ClaudePluginInspectionError::Json {
                path: path.to_path_buf(),
                source,
            }
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ClaudePluginInspectionError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, ClaudePluginInspectionError> {
    let bytes = std::fs::read(path).map_err(|source| ClaudePluginInspectionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ClaudePluginInspectionError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn split_plugin_id(plugin_id: &str) -> Result<(&str, &str), ClaudePluginInspectionError> {
    let segments = plugin_id.split('@').collect::<Vec<_>>();
    if segments.len() != 2 || segments.iter().any(|segment| !valid_segment(segment)) {
        return Err(ClaudePluginInspectionError::InvalidPluginId(
            plugin_id.to_string(),
        ));
    }
    Ok((segments[0], segments[1]))
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn residual_reasons(components: &PluginComponents) -> Vec<String> {
    let mut reasons = Vec::new();
    if components.lsp_servers.is_some() {
        reasons.push("Codex has no native Claude LSP Plugin component".into());
    }
    if !components.unknown.is_empty() {
        reasons.push(format!(
            "Unknown Claude Plugin components: {}",
            components.unknown.join(", ")
        ));
    }
    if !components.hooks.is_empty() {
        reasons.push("Claude Hook components require a separate verified mapping".into());
    }
    if !components.apps.is_empty() {
        reasons.push("Claude App components require a separate verified mapping".into());
    }
    if !components.agents.is_empty() {
        reasons.push("Claude Agent components require a separate verified mapping".into());
    }
    reasons
}

#[derive(Debug, Deserialize)]
struct InstalledRegistry {
    #[serde(default)]
    plugins: BTreeMap<String, Vec<InstalledEntry>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledEntry {
    scope: String,
    #[serde(default)]
    project_path: Option<PathBuf>,
    install_path: PathBuf,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnownMarketplace {
    source: KnownMarketplaceSource,
    install_location: PathBuf,
}

#[derive(Debug, Deserialize)]
struct KnownMarketplaceSource {
    #[serde(rename = "source")]
    kind: String,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudePluginInspectionError {
    #[error("invalid Claude Plugin id: {0}")]
    InvalidPluginId(String),
    #[error("Claude Plugin is not installed: {0}")]
    PluginNotInstalled(String),
    #[error("Claude Plugin has no package for the active project or user scope: {0}")]
    NoProjectPackage(String),
    #[error("Claude Plugin marketplace is not registered: {0}")]
    MarketplaceNotFound(String),
    #[error("invalid Claude Plugin marketplace: {0}")]
    InvalidMarketplace(String),
    #[error("unsupported Claude Plugin marketplace source: {0}")]
    UnsupportedMarketplace(String),
    #[error("invalid Claude Plugin package: {0}")]
    InvalidPackage(String),
    #[error("Plugin manifest name/version does not match its installed registry entry")]
    ManifestIdentityMismatch,
    #[error("Claude Plugin route cannot produce an installable Codex package: {0:?}")]
    UnsupportedRoute(ClaudePluginRoute),
    #[error("failed to prepare Project Plugin staging: {0}")]
    Stage(String),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

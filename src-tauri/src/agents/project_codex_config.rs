use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ContentDigest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceOverlay {
    pub source_type: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPluginOverlay {
    #[serde(default)]
    pub marketplaces: BTreeMap<String, MarketplaceOverlay>,
    #[serde(default)]
    pub enabled_plugins: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSynthesisResult {
    pub content: String,
    pub base_config_digest: Option<ContentDigest>,
    pub generated_config_digest: ContentDigest,
}

pub fn synthesize_project_codex_config(
    base_config: Option<&[u8]>,
    base_config_dir: &Path,
    overlay: &ProjectPluginOverlay,
) -> Result<ConfigSynthesisResult, ConfigSynthesisError> {
    synthesize_project_codex_config_with_settings(
        base_config,
        base_config_dir,
        overlay,
        &BTreeMap::new(),
    )
}

pub fn synthesize_project_codex_config_with_settings(
    base_config: Option<&[u8]>,
    base_config_dir: &Path,
    overlay: &ProjectPluginOverlay,
    project_settings: &BTreeMap<String, toml::Value>,
) -> Result<ConfigSynthesisResult, ConfigSynthesisError> {
    let mut root = match base_config {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| ConfigSynthesisError::InvalidBase(error.to_string()))?
            .parse::<toml::Value>()
            .map_err(|error| ConfigSynthesisError::InvalidBase(error.to_string()))?,
        None => toml::Value::Table(toml::map::Map::new()),
    };
    let table = root
        .as_table_mut()
        .ok_or(ConfigSynthesisError::BaseMustBeTable)?;

    normalize_base_paths(table, base_config_dir)?;
    merge_project_settings(table, project_settings)?;
    table.insert(
        "cli_auth_credentials_store".to_string(),
        toml::Value::String("file".to_string()),
    );
    merge_marketplaces(table, overlay)?;
    merge_plugins(table, overlay)?;

    let mut content = toml::to_string_pretty(&root)
        .map_err(|error| ConfigSynthesisError::Serialization(error.to_string()))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    Ok(ConfigSynthesisResult {
        base_config_digest: base_config.map(ContentDigest::sha256),
        generated_config_digest: ContentDigest::sha256(content.as_bytes()),
        content,
    })
}

fn merge_project_settings(
    root: &mut toml::map::Map<String, toml::Value>,
    project_settings: &BTreeMap<String, toml::Value>,
) -> Result<(), ConfigSynthesisError> {
    for (key, value) in project_settings {
        if matches!(
            key.as_str(),
            "cli_auth_credentials_store" | "marketplaces" | "plugins"
        ) {
            return Err(ConfigSynthesisError::InvalidProjectSetting(key.clone()));
        }
        root.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn merge_marketplaces(
    root: &mut toml::map::Map<String, toml::Value>,
    overlay: &ProjectPluginOverlay,
) -> Result<(), ConfigSynthesisError> {
    if overlay.marketplaces.is_empty() {
        return Ok(());
    }
    let marketplaces = table_entry(root, "marketplaces")?;
    for (name, incoming) in &overlay.marketplaces {
        validate_segment(name, "marketplace")?;
        if incoming.source_type != "git" && incoming.source_type != "local" {
            return Err(ConfigSynthesisError::InvalidMarketplace(format!(
                "unsupported source type `{}` for marketplace `{name}`",
                incoming.source_type
            )));
        }
        if incoming.source.trim().is_empty() {
            return Err(ConfigSynthesisError::InvalidMarketplace(format!(
                "marketplace `{name}` has an empty source"
            )));
        }

        let marketplace = marketplaces
            .entry(name.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                ConfigSynthesisError::InvalidMarketplace(format!(
                    "marketplace `{name}` must be a table"
                ))
            })?;
        if !marketplace.is_empty() && !marketplace_matches(marketplace, incoming) {
            return Err(ConfigSynthesisError::MarketplaceConflict(name.clone()));
        }
        marketplace.insert(
            "source_type".to_string(),
            toml::Value::String(incoming.source_type.clone()),
        );
        marketplace.insert(
            "source".to_string(),
            toml::Value::String(incoming.source.clone()),
        );
        set_optional_string(marketplace, "ref", incoming.ref_name.as_deref());
        set_optional_string(
            marketplace,
            "last_revision",
            incoming.last_revision.as_deref(),
        );
    }
    Ok(())
}

fn merge_plugins(
    root: &mut toml::map::Map<String, toml::Value>,
    overlay: &ProjectPluginOverlay,
) -> Result<(), ConfigSynthesisError> {
    if overlay.enabled_plugins.is_empty() {
        return Ok(());
    }
    let plugins = table_entry(root, "plugins")?;
    for (plugin_id, enabled) in &overlay.enabled_plugins {
        validate_plugin_id(plugin_id)?;
        let plugin = plugins
            .entry(plugin_id.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| ConfigSynthesisError::InvalidPlugin(plugin_id.clone()))?;
        plugin.insert("enabled".to_string(), toml::Value::Boolean(*enabled));
    }
    Ok(())
}

fn normalize_base_paths(
    root: &mut toml::map::Map<String, toml::Value>,
    base_config_dir: &Path,
) -> Result<(), ConfigSynthesisError> {
    normalize_path_field(root, "sqlite_home", base_config_dir)?;

    if let Some(agents) = root.get_mut("agents").and_then(toml::Value::as_table_mut) {
        for (_, agent) in agents.iter_mut() {
            let table = agent
                .as_table_mut()
                .ok_or_else(|| ConfigSynthesisError::InvalidPathField("agents".to_string()))?;
            normalize_path_field(table, "config_file", base_config_dir)?;
        }
    }

    if let Some(servers) = root
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
    {
        for (_, server) in servers.iter_mut() {
            let table = server
                .as_table_mut()
                .ok_or_else(|| ConfigSynthesisError::InvalidPathField("mcp_servers".to_string()))?;
            normalize_path_field(table, "cwd", base_config_dir)?;
        }
    }

    if let Some(marketplaces) = root
        .get_mut("marketplaces")
        .and_then(toml::Value::as_table_mut)
    {
        for (_, marketplace) in marketplaces.iter_mut() {
            let table = marketplace.as_table_mut().ok_or_else(|| {
                ConfigSynthesisError::InvalidPathField("marketplaces".to_string())
            })?;
            if table.get("source_type").and_then(toml::Value::as_str) == Some("local") {
                normalize_path_field(table, "source", base_config_dir)?;
            }
        }
    }

    if let Some(configs) = root
        .get_mut("skills")
        .and_then(toml::Value::as_table_mut)
        .and_then(|skills| skills.get_mut("config"))
        .and_then(toml::Value::as_array_mut)
    {
        for config in configs {
            let table = config.as_table_mut().ok_or_else(|| {
                ConfigSynthesisError::InvalidPathField("skills.config".to_string())
            })?;
            normalize_path_field(table, "path", base_config_dir)?;
        }
    }
    Ok(())
}

fn normalize_path_field(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    base_config_dir: &Path,
) -> Result<(), ConfigSynthesisError> {
    let Some(value) = table.get_mut(key) else {
        return Ok(());
    };
    let path = value
        .as_str()
        .ok_or_else(|| ConfigSynthesisError::InvalidPathField(key.to_string()))?;
    if path.is_empty() || Path::new(path).is_absolute() {
        return Ok(());
    }
    *value = toml::Value::String(
        base_config_dir
            .join(PathBuf::from(path))
            .to_string_lossy()
            .into_owned(),
    );
    Ok(())
}

fn marketplace_matches(
    existing: &toml::map::Map<String, toml::Value>,
    incoming: &MarketplaceOverlay,
) -> bool {
    existing.get("source_type").and_then(toml::Value::as_str) == Some(incoming.source_type.as_str())
        && existing.get("source").and_then(toml::Value::as_str) == Some(incoming.source.as_str())
        && existing.get("ref").and_then(toml::Value::as_str) == incoming.ref_name.as_deref()
}

fn table_entry<'a>(
    root: &'a mut toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<&'a mut toml::map::Map<String, toml::Value>, ConfigSynthesisError> {
    root.entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| ConfigSynthesisError::InvalidTable(key.to_string()))
}

fn set_optional_string(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    value: Option<&str>,
) {
    match value {
        Some(value) => {
            table.insert(key.to_string(), toml::Value::String(value.to_string()));
        }
        None => {
            table.remove(key);
        }
    }
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), ConfigSynthesisError> {
    let mut segments = plugin_id.split('@');
    let plugin = segments.next().unwrap_or_default();
    let marketplace = segments.next().unwrap_or_default();
    if segments.next().is_some() {
        return Err(ConfigSynthesisError::InvalidPlugin(plugin_id.to_string()));
    }
    validate_segment(plugin, "plugin")?;
    validate_segment(marketplace, "marketplace")?;
    Ok(())
}

fn validate_segment(segment: &str, kind: &str) -> Result<(), ConfigSynthesisError> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || !segment
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ConfigSynthesisError::InvalidSegment {
            kind: kind.to_string(),
            value: segment.to_string(),
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigSynthesisError {
    #[error("invalid base Codex config: {0}")]
    InvalidBase(String),
    #[error("base Codex config must be a TOML table")]
    BaseMustBeTable,
    #[error("invalid Codex config table: {0}")]
    InvalidTable(String),
    #[error("invalid path-bearing Codex config field: {0}")]
    InvalidPathField(String),
    #[error("invalid {kind} segment: {value}")]
    InvalidSegment { kind: String, value: String },
    #[error("invalid marketplace: {0}")]
    InvalidMarketplace(String),
    #[error("marketplace `{0}` already exists with a different source")]
    MarketplaceConflict(String),
    #[error("invalid plugin id or config: {0}")]
    InvalidPlugin(String),
    #[error("project setting conflicts with an AD-managed config field: {0}")]
    InvalidProjectSetting(String),
    #[error("failed to serialize generated Codex config: {0}")]
    Serialization(String),
}

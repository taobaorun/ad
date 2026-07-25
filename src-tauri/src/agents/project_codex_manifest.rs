use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde::{Deserialize, Deserializer, Serialize};

use super::{
    ContentDigest, MarketplaceOverlay, ProjectCodexRuntime, ProjectCodexRuntimeError,
    ProjectPluginOverlay,
};

pub const PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub const PROJECT_CODEX_RUNTIME_MANIFEST_MAX_COLLECTION_ENTRIES: usize = 512;
const PROJECT_CODEX_RUNTIME_MANIFEST_MAX_STRING_BYTES: usize = 4 * 1024;
const PROJECT_CODEX_RUNTIME_PROFILE_MAX_BYTES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCodexRuntimeManifest {
    pub schema_version: u32,
    pub applied_inherit_base_config: bool,
    pub applied_profile_id: Option<String>,
    #[serde(deserialize_with = "deserialize_project_overlay")]
    pub project_overlay: ProjectPluginOverlay,
    #[serde(default)]
    pub project_settings_keys: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StrictProjectPluginOverlay {
    #[serde(default)]
    marketplaces: BTreeMap<String, StrictMarketplaceOverlay>,
    #[serde(default)]
    enabled_plugins: BTreeMap<String, bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StrictMarketplaceOverlay {
    source_type: String,
    source: String,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    last_revision: Option<String>,
}

fn deserialize_project_overlay<'de, D>(deserializer: D) -> Result<ProjectPluginOverlay, D::Error>
where
    D: Deserializer<'de>,
{
    let overlay = StrictProjectPluginOverlay::deserialize(deserializer)?;
    Ok(ProjectPluginOverlay {
        marketplaces: overlay
            .marketplaces
            .into_iter()
            .map(|(name, marketplace)| {
                (
                    name,
                    MarketplaceOverlay {
                        source_type: marketplace.source_type,
                        source: marketplace.source,
                        ref_name: marketplace.ref_name,
                        last_revision: marketplace.last_revision,
                    },
                )
            })
            .collect(),
        enabled_plugins: overlay.enabled_plugins,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCodexRuntimeManifestSnapshot {
    pub manifest: ProjectCodexRuntimeManifest,
    pub digest: ContentDigest,
}

pub fn load_project_codex_runtime_manifest(
    runtime: &ProjectCodexRuntime,
) -> Result<Option<ProjectCodexRuntimeManifestSnapshot>, ProjectCodexRuntimeError> {
    let path = runtime.manifest_path();
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ProjectCodexRuntimeError::Io { path, source }),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex runtime manifest must be a physical file".into(),
        ));
    }
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if size > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES {
        return Err(ProjectCodexRuntimeError::ManifestTooLarge {
            size,
            maximum: PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES,
        });
    }
    let bytes = fs::read(&path).map_err(|source| ProjectCodexRuntimeError::Io { path, source })?;
    if bytes.len() > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES {
        return Err(ProjectCodexRuntimeError::ManifestTooLarge {
            size: bytes.len(),
            maximum: PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES,
        });
    }
    let manifest = serde_json::from_slice::<ProjectCodexRuntimeManifest>(&bytes)
        .map_err(|error| ProjectCodexRuntimeError::InvalidManifest(error.to_string()))?;
    validate_project_codex_runtime_manifest(&manifest)?;
    Ok(Some(ProjectCodexRuntimeManifestSnapshot {
        digest: ContentDigest::sha256(&bytes),
        manifest,
    }))
}

pub fn render_project_codex_runtime_manifest(
    manifest: &ProjectCodexRuntimeManifest,
) -> Result<Vec<u8>, ProjectCodexRuntimeError> {
    validate_project_codex_runtime_manifest(manifest)?;
    let mut bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| ProjectCodexRuntimeError::Serialization(error.to_string()))?;
    bytes.push(b'\n');
    if bytes.len() > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES {
        return Err(ProjectCodexRuntimeError::ManifestTooLarge {
            size: bytes.len(),
            maximum: PROJECT_CODEX_RUNTIME_MANIFEST_MAX_BYTES,
        });
    }
    Ok(bytes)
}

fn validate_project_codex_runtime_manifest(
    manifest: &ProjectCodexRuntimeManifest,
) -> Result<(), ProjectCodexRuntimeError> {
    if manifest.schema_version != PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION {
        return Err(ProjectCodexRuntimeError::UnsupportedManifestVersion(
            manifest.schema_version,
        ));
    }
    if let Some(profile_id) = manifest.applied_profile_id.as_deref() {
        validate_profile_id(profile_id)?;
    }
    validate_collection_size("marketplaces", manifest.project_overlay.marketplaces.len())?;
    validate_collection_size(
        "enabled plugins",
        manifest.project_overlay.enabled_plugins.len(),
    )?;
    validate_collection_size("project settings", manifest.project_settings_keys.len())?;
    for (name, marketplace) in &manifest.project_overlay.marketplaces {
        validate_segment(name, "marketplace")?;
        validate_marketplace(marketplace)?;
    }
    for plugin_id in manifest.project_overlay.enabled_plugins.keys() {
        validate_plugin_id(plugin_id)?;
    }
    for setting_key in &manifest.project_settings_keys {
        validate_segment(setting_key, "setting")?;
    }
    Ok(())
}

fn validate_profile_id(profile_id: &str) -> Result<(), ProjectCodexRuntimeError> {
    if profile_id.is_empty()
        || profile_id.len() > PROJECT_CODEX_RUNTIME_PROFILE_MAX_BYTES
        || !profile_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex runtime profile id is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_marketplace(marketplace: &MarketplaceOverlay) -> Result<(), ProjectCodexRuntimeError> {
    if !matches!(marketplace.source_type.as_str(), "git" | "local") {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex marketplace source type must be git or local".into(),
        ));
    }
    validate_string(&marketplace.source, "marketplace source")?;
    if marketplace.source.trim().is_empty() {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex marketplace source must not be empty".into(),
        ));
    }
    validate_optional_string(marketplace.ref_name.as_deref(), "marketplace ref")?;
    validate_optional_string(marketplace.last_revision.as_deref(), "marketplace revision")?;
    reject_credential_bearing_url(&marketplace.source)
}

fn reject_credential_bearing_url(source: &str) -> Result<(), ProjectCodexRuntimeError> {
    let Ok(url) = url::Url::parse(source) else {
        return Ok(());
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex runtime manifest must not contain URL credentials".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex runtime manifest URLs must not contain queries or fragments".into(),
        ));
    }
    Ok(())
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), ProjectCodexRuntimeError> {
    let Some((plugin, marketplace)) = plugin_id.split_once('@') else {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex Plugin id must use <plugin>@<marketplace>".into(),
        ));
    };
    if marketplace.contains('@') {
        return Err(ProjectCodexRuntimeError::InvalidManifest(
            "Project Codex Plugin id must use <plugin>@<marketplace>".into(),
        ));
    }
    validate_segment(plugin, "plugin")?;
    validate_segment(marketplace, "marketplace")
}

fn validate_segment(value: &str, kind: &str) -> Result<(), ProjectCodexRuntimeError> {
    if value.is_empty()
        || value.len() > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_STRING_BYTES
        || value == "."
        || value == ".."
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(ProjectCodexRuntimeError::InvalidManifest(format!(
            "Project Codex {kind} id is invalid"
        )));
    }
    Ok(())
}

fn validate_optional_string(
    value: Option<&str>,
    field: &str,
) -> Result<(), ProjectCodexRuntimeError> {
    if let Some(value) = value {
        validate_string(value, field)?;
    }
    Ok(())
}

fn validate_string(value: &str, field: &str) -> Result<(), ProjectCodexRuntimeError> {
    if value.len() > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_STRING_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ProjectCodexRuntimeError::InvalidManifest(format!(
            "Project Codex {field} is invalid"
        )));
    }
    Ok(())
}

fn validate_collection_size(collection: &str, size: usize) -> Result<(), ProjectCodexRuntimeError> {
    if size > PROJECT_CODEX_RUNTIME_MANIFEST_MAX_COLLECTION_ENTRIES {
        return Err(ProjectCodexRuntimeError::InvalidManifest(format!(
            "Project Codex runtime manifest has too many {collection}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_source(source: &str) -> ProjectCodexRuntimeManifest {
        ProjectCodexRuntimeManifest {
            schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
            applied_inherit_base_config: false,
            applied_profile_id: None,
            project_overlay: ProjectPluginOverlay {
                marketplaces: BTreeMap::from([(
                    "team".into(),
                    MarketplaceOverlay {
                        source_type: "git".into(),
                        source: source.into(),
                        ref_name: None,
                        last_revision: None,
                    },
                )]),
                enabled_plugins: BTreeMap::from([("review@team".into(), true)]),
            },
            project_settings_keys: BTreeSet::new(),
        }
    }

    #[test]
    fn rejects_url_queries_and_fragments() {
        for source in [
            "https://gitlab.example/team.git?oauth_token=secret",
            "https://gitlab.example/team.git?private_token=secret",
            "https://s3.example/team.git?X-Amz-Credential=secret",
            "https://gitlab.example/team.git#access_token=secret",
        ] {
            assert!(render_project_codex_runtime_manifest(&manifest_with_source(source)).is_err());
        }
    }

    #[test]
    fn rejects_unknown_nested_fields() {
        let unknown_overlay = serde_json::json!({
            "schemaVersion": 1,
            "appliedInheritBaseConfig": false,
            "appliedProfileId": null,
            "projectOverlay": {
                "marketplaces": {},
                "enabledPlugins": {},
                "authToken": "secret"
            },
            "projectSettingsKeys": []
        });
        assert!(serde_json::from_value::<ProjectCodexRuntimeManifest>(unknown_overlay).is_err());

        let unknown_marketplace = serde_json::json!({
            "schemaVersion": 1,
            "appliedInheritBaseConfig": false,
            "appliedProfileId": null,
            "projectOverlay": {
                "marketplaces": {
                    "team": {
                        "sourceType": "git",
                        "source": "https://gitlab.example/team.git",
                        "password": "secret"
                    }
                },
                "enabledPlugins": {}
            },
            "projectSettingsKeys": []
        });
        assert!(
            serde_json::from_value::<ProjectCodexRuntimeManifest>(unknown_marketplace).is_err()
        );
    }

    #[test]
    fn rejects_invalid_project_setting_provenance_keys() {
        let mut manifest = manifest_with_source("https://gitlab.example/team.git");
        manifest.project_settings_keys.insert("invalid/key".into());

        assert!(render_project_codex_runtime_manifest(&manifest).is_err());
    }
}

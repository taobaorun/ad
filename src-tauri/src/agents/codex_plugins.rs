use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::fs::paths::projects_state_path;
use chrono::{Duration, Utc};
use semver::Version;
use serde::Deserialize;

use super::codex::discover_codex_candidates;
use super::codex_ports::{
    agent_error, project_runtime_for_context, read_optional, resolve_codex_home,
};
use super::execution_fs::{observe_target, TargetState};
use super::{
    load_project_codex_runtime_manifest, render_project_codex_runtime_manifest,
    synthesize_project_codex_config_with_settings, AgentContext, AgentError, AgentErrorCode,
    AgentId, CapabilityAvailability, CapabilityLimitation, CapabilityOperation,
    CollectionInstallRequest, ContentDigest, ManagedResourceTarget, MarketplaceOverlay,
    MutationKind, MutationPlan, PlanId, PlannedMutation, PluginInstallProgressReporter,
    PluginsPort, ProjectCodexRuntimeManifest, ProjectPluginOverlay, ReadPrecondition, ResourceKind,
    ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef, ResourceScope, ResourceSnapshot,
    ResourceStateKind, SettingsEdit, SharedAuthBinding, WritePolicy,
    PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexCatalogPluginMetadata {
    pub plugin_name: String,
    pub marketplace_name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
struct CodexPluginDescriptor {
    name: String,
    version: String,
}

pub(super) fn read_codex_catalog_plugin_metadata(
    root: &Path,
    expected_name: &str,
) -> Result<CodexCatalogPluginMetadata, String> {
    let descriptor_path = root.join(".codex-plugin/plugin.json");
    let descriptor: CodexPluginDescriptor = serde_json::from_slice(
        &std::fs::read(&descriptor_path)
            .map_err(|error| format!("Failed to read {}: {error}", descriptor_path.display()))?,
    )
    .map_err(|error| format!("Invalid {}: {error}", descriptor_path.display()))?;
    if descriptor.name != expected_name
        || !valid_segment(&descriptor.name)
        || !valid_segment(&descriptor.version)
    {
        return Err("Codex Plugin descriptor identity is invalid".into());
    }

    let marketplace_path = root.join(".agents/plugins/marketplace.json");
    let marketplace: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&marketplace_path)
            .map_err(|error| format!("Failed to read {}: {error}", marketplace_path.display()))?,
    )
    .map_err(|error| format!("Invalid {}: {error}", marketplace_path.display()))?;
    let marketplace_name = marketplace
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_segment(name))
        .ok_or_else(|| "Codex marketplace has no valid name".to_string())?;
    let plugin = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .and_then(|plugins| {
            plugins.iter().find(|plugin| {
                plugin.get("name").and_then(serde_json::Value::as_str)
                    == Some(descriptor.name.as_str())
            })
        })
        .ok_or_else(|| "Codex marketplace does not declare this Plugin".to_string())?;
    let source_is_root = match plugin.get("source") {
        Some(serde_json::Value::String(path)) => matches!(path.as_str(), "." | "./"),
        Some(serde_json::Value::Object(source)) => {
            source.get("source").and_then(serde_json::Value::as_str) == Some("local")
                && source
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|path| matches!(path, "." | "./"))
        }
        _ => false,
    };
    if !source_is_root {
        return Err(
            "Codex marketplace Plugin source must reference the managed resource root".into(),
        );
    }

    Ok(CodexCatalogPluginMetadata {
        plugin_name: descriptor.name,
        marketplace_name: marketplace_name.to_owned(),
        version: descriptor.version,
    })
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

#[derive(Debug, Default)]
pub(crate) struct CodexPluginsPort;

pub(super) struct ProjectRuntimeBootstrapPlan {
    pub(super) plan: MutationPlan,
}

pub(super) fn plan_project_runtime_bootstrap(
    context: &AgentContext,
    inherit_base_config: bool,
    profile_id: Option<&str>,
    _report: &PluginInstallProgressReporter<'_>,
) -> Result<Option<ProjectRuntimeBootstrapPlan>, AgentError> {
    let Some(runtime) = project_runtime_for_context(context)? else {
        return Ok(None);
    };
    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), context, None)?;
    let credential_store = base_config
        .as_deref()
        .map(|bytes| parse_credential_store(context, bytes))
        .transpose()?
        .flatten();
    let auth = SharedAuthBinding::detect(
        &base_home,
        &runtime.runtime_home,
        credential_store.as_deref(),
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })?;
    let (auth_source, auth_target) = reusable_auth_paths(context, auth)?;
    // Runtime bootstrap only synthesizes shared state. Catalog Plugin install
    // plans add their own digest-protected package mutations.
    let config_resource = project_resource(context, "runtime-config");
    let config_target = ManagedResourceTarget::file(runtime.runtime_home.join("config.toml"));
    let config_state = observe_target(&config_target)?;
    validate_runtime_config_state(context, &runtime, &config_resource, &config_state)?;
    let (overlay, project_settings_keys) =
        project_overlays_for_plan(context, &runtime, &config_state, inherit_base_config)?;
    let project_settings =
        project_settings_from_config(context, &config_state, &project_settings_keys)?;
    let inherited_config = inherit_base_config
        .then_some(base_config.as_deref())
        .flatten();
    let synthesized = synthesize_project_codex_config_with_settings(
        inherited_config,
        &base_home,
        &overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })?;
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: inherit_base_config,
        applied_profile_id: profile_id.map(str::to_owned),
        project_overlay: overlay,
        project_settings_keys,
    };
    let manifest_resource = project_resource(context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(context, &runtime, &manifest_resource, &manifest_state)?;

    let auth_resource = project_resource(context, "runtime-auth");
    let auth_state = observe_target(&ManagedResourceTarget::symlink(auth_target))?;
    if !matches!(auth_state, TargetState::Missing | TargetState::Symlink(_)) {
        return Err(storage_conflict(context, &auth_resource));
    }
    let mut read_set = Vec::new();
    if let Some(digest) = base_config.as_deref().map(ContentDigest::sha256) {
        read_set.push(ReadPrecondition {
            resource: project_resource(context, "base-config"),
            expected_digest: digest,
            write_policy: WritePolicy::ReadOnly,
        });
    }
    let mut mutations = Vec::new();
    push_symlink_mutation(&mut mutations, auth_resource, &auth_state, &auth_source);
    push_manifest_mutation(
        context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    if config_state.digest().as_ref() != Some(&synthesized.generated_config_digest) {
        mutations.push(PlannedMutation {
            resource: config_resource,
            kind: mutation_kind(&config_state),
            expected_digest: config_state.digest(),
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(synthesized.content)),
        });
    }

    Ok(Some(ProjectRuntimeBootstrapPlan {
        plan: MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("codex"),
            context: context.clone(),
            read_set,
            mutations,
            expires_at: Utc::now() + Duration::minutes(5),
        },
    }))
}

pub(crate) fn plan_project_runtime_profile_apply(
    context: &AgentContext,
    profile_id: &str,
    config_toml: &str,
) -> Result<Option<MutationPlan>, AgentError> {
    let Some(runtime) = project_runtime_for_context(context)? else {
        return Ok(None);
    };
    let config_resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Settings,
        scope: ResourceScope::Project,
        logical_id: "runtime-config".into(),
    };
    let config_state = observe_target(&ManagedResourceTarget::file(
        runtime.runtime_home.join("config.toml"),
    ))?;
    validate_runtime_config_state(context, &runtime, &config_resource, &config_state)?;

    let manifest_resource = project_resource(context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(context, &runtime, &manifest_resource, &manifest_state)?;
    let snapshot = load_project_codex_runtime_manifest(&runtime)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(manifest_resource.clone()),
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(manifest_resource.clone()),
                "Project Codex runtime needs Preview and Apply before Profile changes",
            )
        })?;
    if !snapshot.manifest.applied_inherit_base_config {
        return Err(agent_error(
            AgentErrorCode::Unsupported,
            context,
            Some(config_resource),
            "Project Codex Profile application requires Base config inheritance",
        ));
    }

    let profile = config_toml.parse::<toml::Value>().map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(config_resource.clone()),
            format!("Invalid Codex Profile TOML: {error}"),
        )
    })?;
    let project_settings = profile
        .as_table()
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(config_resource.clone()),
                "Codex Profile config must be a TOML table",
            )
        })?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let project_settings_keys = project_settings.keys().cloned().collect();
    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), context, None)?;
    let synthesized = synthesize_project_codex_config_with_settings(
        base_config.as_deref(),
        &base_home,
        &snapshot.manifest.project_overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(config_resource.clone()),
            error.to_string(),
        )
    })?;
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: true,
        applied_profile_id: Some(profile_id.to_owned()),
        project_overlay: snapshot.manifest.project_overlay,
        project_settings_keys,
    };
    let mut mutations = Vec::new();
    push_manifest_mutation(
        context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    if config_state.digest().as_ref() != Some(&synthesized.generated_config_digest) {
        mutations.push(PlannedMutation {
            resource: config_resource,
            kind: mutation_kind(&config_state),
            expected_digest: config_state.digest(),
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(synthesized.content)),
        });
    }
    let read_set = base_config
        .as_deref()
        .map(ContentDigest::sha256)
        .map(|expected_digest| {
            vec![ReadPrecondition {
                resource: project_resource(context, "base-config"),
                expected_digest,
                write_policy: WritePolicy::ReadOnly,
            }]
        })
        .unwrap_or_default();
    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    };
    plan.validate()?;
    Ok(Some(plan))
}

pub(super) fn plan_project_runtime_settings_edit(
    context: &AgentContext,
    edit: &SettingsEdit,
) -> Result<Option<MutationPlan>, AgentError> {
    let Some(runtime) = project_runtime_for_context(context)? else {
        return Ok(None);
    };
    if edit.resource.kind != ResourceKind::Settings
        || edit.resource.scope != ResourceScope::Project
        || edit.resource.logical_id != "runtime-config"
        || edit.resource.installation_id != context.installation_id
        || edit.resource.project_path != context.project_path
    {
        return Ok(None);
    }
    let proposed = edit
        .content
        .as_str()
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                "Project Codex runtime settings must be TOML text",
            )
        })?
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                format!("Invalid Project Codex runtime TOML: {error}"),
            )
        })?;
    let proposed_root = proposed.as_table().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            "Project Codex runtime config must be a TOML table",
        )
    })?;

    let config_target = ManagedResourceTarget::file(runtime.runtime_home.join("config.toml"));
    let config_state = observe_target(&config_target)?;
    validate_runtime_config_state(context, &runtime, &edit.resource, &config_state)?;
    let current = match &config_state {
        TargetState::File(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(edit.resource.clone()),
                    error.to_string(),
                )
            })?
            .parse::<toml::Value>()
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(edit.resource.clone()),
                    error.to_string(),
                )
            })?,
        TargetState::Missing => toml::Value::Table(toml::map::Map::new()),
        _ => return Err(storage_conflict(context, &edit.resource)),
    };
    let current_root = current.as_table().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            "Existing Project Codex runtime config must be a TOML table",
        )
    })?;

    let manifest_resource = project_resource(context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(context, &runtime, &manifest_resource, &manifest_state)?;
    let snapshot = load_project_codex_runtime_manifest(&runtime).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(manifest_resource.clone()),
            error.to_string(),
        )
    })?;
    let Some(snapshot) = snapshot else {
        // Initial and legacy conversion previews merge their settings mutation with
        // a bootstrap manifest later in the route. Only an applied, manifest-backed
        // runtime can update provenance atomically at this layer.
        return Ok(None);
    };

    let mut project_settings = project_settings_from_config(
        context,
        &config_state,
        &snapshot.manifest.project_settings_keys,
    )?;
    project_settings.retain(|key, _| proposed_root.contains_key(key));
    for (key, value) in proposed_root {
        if matches!(
            key.as_str(),
            "cli_auth_credentials_store" | "marketplaces" | "plugins"
        ) {
            continue;
        }
        if project_settings.contains_key(key) || current_root.get(key) != Some(value) {
            project_settings.insert(key.clone(), value.clone());
        }
    }

    let inherit_base_config = snapshot.manifest.applied_inherit_base_config;
    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = if inherit_base_config {
        read_optional(&base_home.join("config.toml"), context, None)?
    } else {
        None
    };
    let synthesized = synthesize_project_codex_config_with_settings(
        base_config.as_deref(),
        &base_home,
        &snapshot.manifest.project_overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            error.to_string(),
        )
    })?;
    let synthesized_value = synthesized
        .content
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                error.to_string(),
            )
        })?;
    if synthesized_value != proposed {
        return Err(agent_error(
            AgentErrorCode::Unsupported,
            context,
            Some(edit.resource.clone()),
            "Project Codex runtime edits cannot remove inherited settings or directly change AD-managed auth, marketplace, or Plugin fields",
        ));
    }

    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: inherit_base_config,
        applied_profile_id: snapshot.manifest.applied_profile_id,
        project_overlay: snapshot.manifest.project_overlay,
        project_settings_keys: project_settings.keys().cloned().collect(),
    };
    let mut mutations = Vec::new();
    push_manifest_mutation(
        context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    mutations.push(PlannedMutation {
        resource: edit.resource.clone(),
        kind: mutation_kind(&config_state),
        expected_digest: config_state.digest(),
        media_type: "application/toml".into(),
        content: Some(serde_json::Value::String(synthesized.content)),
    });
    let read_set = base_config
        .as_deref()
        .map(ContentDigest::sha256)
        .map(|expected_digest| {
            vec![ReadPrecondition {
                resource: project_resource(context, "base-config"),
                expected_digest,
                write_policy: WritePolicy::ReadOnly,
            }]
        })
        .unwrap_or_default();
    let plan = MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    };
    plan.validate()?;
    Ok(Some(plan))
}

pub(super) fn plan_project_runtime_semantic_settings_edit(
    context: &AgentContext,
    edit: &SettingsEdit,
) -> Result<Option<MutationPlan>, AgentError> {
    let Some(runtime) = project_runtime_for_context(context)? else {
        return Ok(None);
    };
    if edit.resource.kind != ResourceKind::Settings
        || edit.resource.scope != ResourceScope::Project
        || edit.resource.logical_id != "runtime-config"
        || edit.resource.installation_id != context.installation_id
        || edit.resource.project_path != context.project_path
    {
        return Ok(None);
    }
    let mut proposed = edit.content.clone();
    if !proposed.is_object() {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            "Project Codex settings must be a JSON object",
        ));
    }
    let config_target = ManagedResourceTarget::file(runtime.runtime_home.join("config.toml"));
    let config_state = observe_target(&config_target)?;
    validate_runtime_config_state(context, &runtime, &edit.resource, &config_state)?;
    let manifest_resource = project_resource(context, "runtime-manifest");
    let snapshot = load_project_codex_runtime_manifest(&runtime)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(manifest_resource),
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                "Prepared Project Runtime manifest is missing",
            )
        })?;
    let current_settings = project_settings_from_config(
        context,
        &config_state,
        &snapshot.manifest.project_settings_keys,
    )?;
    let current_json = serde_json::to_value(&current_settings).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            error.to_string(),
        )
    })?;
    super::settings_inventory::restore_masked_settings_values(&mut proposed, &current_json);
    let proposed = proposed.as_object().expect("object shape was validated");
    let mut project_settings = BTreeMap::new();
    for (key, value) in proposed {
        let value = toml::Value::try_from(value.clone()).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(edit.resource.clone()),
                format!("Invalid Project Codex setting {key}: {error}"),
            )
        })?;
        project_settings.insert(key.clone(), value);
    }
    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = if snapshot.manifest.applied_inherit_base_config {
        read_optional(&base_home.join("config.toml"), context, None)?
    } else {
        None
    };
    let synthesized = synthesize_project_codex_config_with_settings(
        base_config.as_deref(),
        &base_home,
        &snapshot.manifest.project_overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(edit.resource.clone()),
            error.to_string(),
        )
    })?;
    plan_project_runtime_settings_edit(
        context,
        &SettingsEdit {
            resource: edit.resource.clone(),
            media_type: "application/toml".into(),
            content: serde_json::Value::String(synthesized.content),
        },
    )
}

impl ResourcePort for CodexPluginsPort {
    fn resolve(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, AgentError> {
        if let Some(runtime) = project_runtime_for_context(context)? {
            validate_project_resource(context, resource)?;
            return resolve_project_resource(context, resource, &runtime);
        }
        validate_user_resource(context, resource)?;
        Ok(ManagedResourceTarget::file(
            resolve_codex_home(context)?.join("config.toml"),
        ))
    }
}

impl PluginsPort for CodexPluginsPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([
            CapabilityOperation::List,
            CapabilityOperation::Install,
            CapabilityOperation::Enable,
            CapabilityOperation::Disable,
            CapabilityOperation::Preview,
            CapabilityOperation::Apply,
            CapabilityOperation::Rollback,
        ])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Degraded
    }

    fn limitations(&self) -> Vec<CapabilityLimitation> {
        vec![CapabilityLimitation {
            code: "user_install_requires_codex_marketplace_flow".into(),
            message_key: "agents.capabilities.codexPluginInstallRequiresMarketplace".into(),
        }]
    }

    fn list(&self, context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
        let config = resolve_codex_home(context)?.join("config.toml");
        let Some(bytes) = read_optional(&config, context, None)? else {
            return Ok(Vec::new());
        };
        let value = std::str::from_utf8(&bytes)
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?
            .parse::<toml::Value>()
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?;
        let Some(plugins) = value.get("plugins").and_then(toml::Value::as_table) else {
            return Ok(Vec::new());
        };
        let project_runtime = project_runtime_for_context(context)?.is_some();
        plugins
            .iter()
            .map(|(id, config_value)| {
                let enabled = config_value
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                let content = serde_json::json!({"enabled": enabled});
                let digest_bytes = serde_json::to_vec(&content).map_err(|error| {
                    agent_error(AgentErrorCode::Io, context, None, error.to_string())
                })?;
                Ok(ResourceSnapshot {
                    resource: ResourceRef {
                        installation_id: context.installation_id.clone(),
                        project_path: project_runtime
                            .then(|| context.project_path.clone())
                            .flatten(),
                        kind: ResourceKind::Plugins,
                        scope: if project_runtime {
                            ResourceScope::Project
                        } else {
                            ResourceScope::User
                        },
                        logical_id: id.clone(),
                    },
                    location: ResourceLocation {
                        path: config.to_string_lossy().into_owned(),
                        origin: if project_runtime {
                            ResourceOrigin::Project
                        } else {
                            ResourceOrigin::User
                        },
                    },
                    media_type: "application/vnd.ad.plugin+json".into(),
                    content,
                    digest: ContentDigest::sha256(&digest_bytes),
                    observed_at: Utc::now(),
                })
            })
            .collect()
    }

    fn plan_install(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
    ) -> Result<MutationPlan, AgentError> {
        plan_catalog_plugin_install(context, request, &|_| {})
    }

    fn plan_install_with_progress(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
        report: &PluginInstallProgressReporter<'_>,
    ) -> Result<MutationPlan, AgentError> {
        plan_catalog_plugin_install(context, request, report)
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        if managed_catalog_identity(&resource.logical_id).is_some() {
            return plan_managed_plugin_change(context, resource, Some(enabled));
        }
        if let Some(runtime) = project_runtime_for_context(context)? {
            validate_project_plugin_resource(context, resource)?;
            return plan_project_override(context, resource, Some(enabled), &runtime);
        }
        validate_user_resource(context, resource)?;
        let config = resolve_codex_home(context)?.join("config.toml");
        let existing = read_optional(&config, context, Some(resource.clone()))?;
        let mut value = match existing.as_deref() {
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|error| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        error.to_string(),
                    )
                })?
                .parse::<toml::Value>()
                .map_err(|error| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        Some(resource.clone()),
                        error.to_string(),
                    )
                })?,
            None => toml::Value::Table(toml::map::Map::new()),
        };
        let root = value.as_table_mut().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                "Codex config must be a TOML table",
            )
        })?;
        let plugins = root
            .entry("plugins")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "plugins must be a TOML table",
                )
            })?;
        let plugin = plugins
            .entry(resource.logical_id.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    "plugin config must be a TOML table",
                )
            })?;
        plugin.insert("enabled".into(), toml::Value::Boolean(enabled));
        let rendered = toml::to_string_pretty(&value).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        })?;
        let expected_digest = existing.as_deref().map(ContentDigest::sha256);
        Ok(MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("codex"),
            context: context.clone(),
            read_set: expected_digest
                .clone()
                .map(|digest| {
                    vec![ReadPrecondition {
                        resource: resource.clone(),
                        expected_digest: digest,
                        write_policy: WritePolicy::Mutable,
                    }]
                })
                .unwrap_or_default(),
            mutations: vec![PlannedMutation {
                resource: resource.clone(),
                kind: if expected_digest.is_some() {
                    MutationKind::Replace
                } else {
                    MutationKind::Create
                },
                expected_digest,
                media_type: "application/toml".into(),
                content: Some(serde_json::Value::String(rendered)),
            }],
            expires_at: Utc::now() + Duration::minutes(5),
        })
    }

    fn plan_remove(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
    ) -> Result<MutationPlan, AgentError> {
        if managed_catalog_identity(&resource.logical_id).is_some() {
            return plan_managed_plugin_change(context, resource, None);
        }
        let runtime = project_runtime_for_context(context)?.ok_or_else(|| {
            agent_error(
                AgentErrorCode::Unsupported,
                context,
                Some(resource.clone()),
                "Codex user Plugin removal requires the Agent marketplace flow",
            )
        })?;
        validate_project_plugin_resource(context, resource)?;
        plan_project_override(context, resource, None, &runtime)
    }
}

fn plan_catalog_plugin_install(
    requested_context: &AgentContext,
    request: CollectionInstallRequest,
    report: &PluginInstallProgressReporter<'_>,
) -> Result<MutationPlan, AgentError> {
    let (context, runtime) = effective_project_runtime(requested_context)?;
    let resource_id = request
        .source
        .get("catalogResourceId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                &context,
                None,
                "Project Plugin installation requires a catalog resource",
            )
        })?;
    let resolved = super::resolve_catalog_resource(resource_id).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            &context,
            None,
            error.to_string(),
        )
    })?;
    if resolved.kind != ResourceKind::Plugins {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            &context,
            None,
            "Catalog resource is not a Plugin",
        ));
    }
    let metadata =
        read_codex_catalog_plugin_metadata(&resolved.physical_path, &resolved.install_id)
            .map_err(|error| agent_error(AgentErrorCode::Unsupported, &context, None, error))?;
    let native_id = format!("{}@{}", metadata.plugin_name, metadata.marketplace_name);
    validate_plugin_id(&context, &native_id)?;

    let package_resource = ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: format!("{}/{}", resolved.source_id, resolved.install_id),
    };
    let package_target = resolve_project_resource(&context, &package_resource, &runtime)?;
    let package_state = observe_target(&package_target)?;
    if !matches!(package_state, TargetState::Missing) {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            &context,
            Some(package_resource),
            "Codex Plugin package target is already occupied",
        ));
    }

    let base_home = base_home(&context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), &context, None)?;
    let credential_store = base_config
        .as_deref()
        .map(|bytes| parse_credential_store(&context, bytes))
        .transpose()?
        .flatten();
    let auth = SharedAuthBinding::detect(
        &base_home,
        &runtime.runtime_home,
        credential_store.as_deref(),
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            &context,
            None,
            error.to_string(),
        )
    })?;
    let (auth_source, auth_target) = reusable_auth_paths(&context, auth)?;

    let config_resource = project_resource(&context, "runtime-config");
    let config_state = observe_target(&ManagedResourceTarget::file(
        runtime.runtime_home.join("config.toml"),
    ))?;
    validate_runtime_config_state(&context, &runtime, &config_resource, &config_state)?;
    let inherit_base_config = desired_inherit_base_config(&context, &runtime)?;
    let (mut overlay, project_settings_keys) =
        project_overlays_for_plan(&context, &runtime, &config_state, inherit_base_config)?;
    let project_settings =
        project_settings_from_config(&context, &config_state, &project_settings_keys)?;
    let marketplace = MarketplaceOverlay {
        source_type: "local".into(),
        source: resolved.stable_path.to_string_lossy().into_owned(),
        ref_name: None,
        last_revision: None,
    };
    if overlay
        .marketplaces
        .get(&metadata.marketplace_name)
        .is_some_and(|existing| existing != &marketplace)
    {
        return Err(agent_error(
            AgentErrorCode::ResourceChanged,
            &context,
            None,
            format!(
                "Project marketplace {} already uses a different source",
                metadata.marketplace_name
            ),
        ));
    }
    overlay
        .marketplaces
        .insert(metadata.marketplace_name.clone(), marketplace);
    overlay.enabled_plugins.insert(native_id.clone(), true);
    let inherited_config = inherit_base_config
        .then_some(base_config.as_deref())
        .flatten();
    let synthesized = synthesize_project_codex_config_with_settings(
        inherited_config,
        &base_home,
        &overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            &context,
            None,
            error.to_string(),
        )
    })?;
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: inherit_base_config,
        applied_profile_id: runtime.profile_id.clone(),
        project_overlay: overlay,
        project_settings_keys,
    };

    let auth_resource = project_resource(&context, "runtime-auth");
    let auth_state = observe_target(&ManagedResourceTarget::symlink(auth_target))?;
    if !matches!(auth_state, TargetState::Missing | TargetState::Symlink(_)) {
        return Err(storage_conflict(&context, &auth_resource));
    }
    let manifest_resource = project_resource(&context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(&context, &runtime, &manifest_resource, &manifest_state)?;
    let package_digest = catalog_plugin_tree_digest(&resolved.stable_path).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            &context,
            Some(package_resource.clone()),
            error.to_string(),
        )
    })?;

    let mut read_set = Vec::new();
    if let Some(expected_digest) = base_config.as_deref().map(ContentDigest::sha256) {
        read_set.push(ReadPrecondition {
            resource: project_resource(&context, "base-config"),
            expected_digest,
            write_policy: WritePolicy::ReadOnly,
        });
    }
    let mut mutations = Vec::new();
    push_symlink_mutation(&mut mutations, auth_resource, &auth_state, &auth_source);
    if inherit_base_config {
        append_inherited_package_mutations(
            &context,
            &runtime,
            &base_home,
            base_config.as_deref(),
            native_id,
            report,
            &mut mutations,
        )?;
    }
    push_catalog_plugin_directory_mutation(
        &mut mutations,
        package_resource,
        &package_state,
        &resolved.stable_path,
        package_digest,
    );
    push_manifest_mutation(
        &context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    if config_state.digest().as_ref() != Some(&synthesized.generated_config_digest) {
        mutations.push(PlannedMutation {
            resource: config_resource,
            kind: mutation_kind(&config_state),
            expected_digest: config_state.digest(),
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(synthesized.content)),
        });
    }

    Ok(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context,
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    })
}

fn append_inherited_package_mutations(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    base_home: &Path,
    base_config: Option<&[u8]>,
    excluded_plugin_id: String,
    report: &PluginInstallProgressReporter<'_>,
    mutations: &mut Vec<PlannedMutation>,
) -> Result<(), AgentError> {
    let Some(base_config) = base_config else {
        return Ok(());
    };
    let value = std::str::from_utf8(base_config)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?;
    let Some(plugins) = value.get("plugins").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    let enabled = plugins
        .iter()
        .filter(|(plugin_id, config)| {
            plugin_id.as_str() != excluded_plugin_id
                && config
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let marketplaces = enabled
        .iter()
        .filter_map(|(plugin_id, _)| {
            plugin_id
                .split_once('@')
                .map(|(_, marketplace)| marketplace)
        })
        .collect::<BTreeSet<_>>();
    let marketplace_configs = value.get("marketplaces").and_then(toml::Value::as_table);
    for marketplace in marketplaces {
        let Some(config) = marketplace_configs
            .and_then(|marketplaces| marketplaces.get(marketplace))
            .and_then(toml::Value::as_table)
        else {
            continue;
        };
        match config.get("source_type").and_then(toml::Value::as_str) {
            Some("local") => {}
            Some("git") => {
                let source = base_home.join(".tmp/marketplaces").join(marketplace);
                let digest = super::directory_tree_digest(&source).map_err(|error| {
                    agent_error(
                        AgentErrorCode::ResourceChanged,
                        context,
                        None,
                        format!("Invalid base marketplace {marketplace}: {error}"),
                    )
                })?;
                let resource = project_resource(context, format!("marketplace:{marketplace}"));
                let target = resolve_project_resource(context, &resource, runtime)?;
                let state = observe_target(&target)?;
                match &state {
                    TargetState::Missing => {
                        push_directory_mutation(mutations, resource, &state, &source, digest)
                    }
                    TargetState::Directory(existing) if existing == &digest => {}
                    TargetState::Directory(_) => {
                        return Err(agent_error(
                            AgentErrorCode::ResourceChanged,
                            context,
                            Some(resource),
                            format!("Inherited Project marketplace {marketplace} has changed"),
                        ))
                    }
                    _ => return Err(storage_conflict(context, &resource)),
                }
            }
            Some(other) => {
                return Err(agent_error(
                    AgentErrorCode::Unsupported,
                    context,
                    None,
                    format!("Unsupported base marketplace source type {other}"),
                ))
            }
            None => {
                return Err(agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Base marketplace {marketplace} has no source type"),
                ))
            }
        }
    }
    for (index, (plugin_id, _)) in enabled.iter().enumerate() {
        report(super::PluginInstallProgress {
            logical_id: (*plugin_id).clone(),
            current: index + 1,
            total: enabled.len(),
        });
        validate_plugin_id(context, plugin_id)?;
        let (plugin, marketplace) = plugin_id.split_once('@').unwrap();
        let source_base = base_home
            .join("plugins/cache")
            .join(marketplace)
            .join(plugin);
        let version = active_plugin_version(&source_base).ok_or_else(|| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                None,
                format!("Enabled base Plugin {plugin_id} has no installed package"),
            )
        })?;
        let source = source_base.join(&version);
        let digest = super::directory_tree_digest(&source).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Invalid base Plugin package {plugin_id}: {error}"),
            )
        })?;
        let resource =
            project_resource(context, format!("package:{marketplace}:{plugin}:{version}"));
        let target = resolve_project_resource(context, &resource, runtime)?;
        let state = observe_target(&target)?;
        match &state {
            TargetState::Missing => {
                push_directory_mutation(mutations, resource, &state, &source, digest)
            }
            TargetState::Directory(existing) if existing == &digest => {}
            TargetState::Directory(_) => {
                return Err(agent_error(
                    AgentErrorCode::ResourceChanged,
                    context,
                    Some(resource),
                    format!("Inherited Project Plugin package {plugin_id} has changed"),
                ))
            }
            _ => return Err(storage_conflict(context, &resource)),
        }
    }
    Ok(())
}

fn active_plugin_version(package_base: &Path) -> Option<String> {
    let mut versions = std::fs::read_dir(package_base)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().ok().is_some_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            Version::parse(&name).ok().map(|version| (version, name))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    versions.pop().map(|(_, name)| name)
}

struct InstalledCodexPlugin {
    installation: super::ResourceInstallationRecord,
    native_id: String,
    marketplace_name: String,
    package_target: PathBuf,
}

fn managed_codex_installation(
    context: &AgentContext,
    resource: &ResourceRef,
    runtime: &super::ProjectCodexRuntime,
) -> Result<InstalledCodexPlugin, AgentError> {
    validate_project_resource(context, resource)?;
    let (_, install_id) = managed_catalog_identity(&resource.logical_id).ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Managed Codex Plugin identity is invalid",
        )
    })?;
    let installation = super::list_resource_installations()
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?
        .into_iter()
        .find(|record| {
            record.effective_installation_id == context.installation_id
                && record.canonical_project_path
                    == context.project_path.as_deref().unwrap_or_default()
                && record.resource_kind == ResourceKind::Plugins
                && record.source_id
                    == managed_catalog_identity(&resource.logical_id)
                        .map(|identity| identity.0)
                        .unwrap_or_default()
                && record.install_id == install_id
                && record.adapter_contract == "codex-plugin-store-v1"
        })
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Managed Codex Plugin installation evidence is unavailable",
            )
        })?;
    let package_target = managed_codex_package_target(context, resource, runtime)?;
    let relative = package_target
        .strip_prefix(runtime.runtime_home.join("plugins/cache"))
        .map_err(|_| {
            agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource.clone()),
                "Managed Codex Plugin target escapes the project runtime cache",
            )
        })?;
    let components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() != 3 || components[1] != install_id {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            Some(resource.clone()),
            "Managed Codex Plugin target identity is invalid",
        ));
    }
    Ok(InstalledCodexPlugin {
        installation,
        native_id: format!("{}@{}", components[1], components[0]),
        marketplace_name: components[0].clone(),
        package_target,
    })
}

fn plan_managed_plugin_change(
    context: &AgentContext,
    resource: &ResourceRef,
    enabled: Option<bool>,
) -> Result<MutationPlan, AgentError> {
    let runtime = project_runtime_for_context(context)?.ok_or_else(|| {
        agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            Some(resource.clone()),
            "Managed Codex Plugin runtime is unavailable",
        )
    })?;
    let installed = managed_codex_installation(context, resource, &runtime)?;
    let config_resource = project_resource(context, "runtime-config");
    let config_state = observe_target(&ManagedResourceTarget::file(
        runtime.runtime_home.join("config.toml"),
    ))?;
    validate_runtime_config_state(context, &runtime, &config_resource, &config_state)?;
    let snapshot = load_project_codex_runtime_manifest(&runtime)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Managed Codex Plugin runtime manifest is unavailable",
            )
        })?;
    let inherit_base_config = snapshot.manifest.applied_inherit_base_config;
    let profile_id = snapshot.manifest.applied_profile_id;
    let project_settings_keys = snapshot.manifest.project_settings_keys;
    let project_settings =
        project_settings_from_config(context, &config_state, &project_settings_keys)?;
    let mut overlay = snapshot.manifest.project_overlay;
    match enabled {
        Some(enabled) => {
            let current = overlay
                .enabled_plugins
                .get_mut(&installed.native_id)
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::ResourceChanged,
                        context,
                        Some(resource.clone()),
                        "Managed Codex Plugin declaration is unavailable",
                    )
                })?;
            *current = enabled;
        }
        None => {
            if overlay
                .enabled_plugins
                .remove(&installed.native_id)
                .is_none()
            {
                return Err(agent_error(
                    AgentErrorCode::ResourceChanged,
                    context,
                    Some(resource.clone()),
                    "Managed Codex Plugin declaration is unavailable",
                ));
            }
            if !overlay.enabled_plugins.keys().any(|plugin_id| {
                plugin_id
                    .split_once('@')
                    .is_some_and(|(_, marketplace)| marketplace == installed.marketplace_name)
            }) {
                overlay.marketplaces.remove(&installed.marketplace_name);
            }
        }
    }

    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), context, None)?;
    let inherited_config = inherit_base_config
        .then_some(base_config.as_deref())
        .flatten();
    let synthesized = synthesize_project_codex_config_with_settings(
        inherited_config,
        &base_home,
        &overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: inherit_base_config,
        applied_profile_id: profile_id,
        project_overlay: overlay,
        project_settings_keys,
    };
    let manifest_resource = project_resource(context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(context, &runtime, &manifest_resource, &manifest_state)?;

    let control_resource = ResourceRef {
        logical_id: format!("plugin-control:{}", installed.installation.id),
        ..resource.clone()
    };
    let control_target = resolve_project_resource(context, &control_resource, &runtime)?;
    let control_state = observe_target(&control_target)?;
    if !matches!(control_state, TargetState::Missing | TargetState::File(_)) {
        return Err(storage_conflict(context, &control_resource));
    }
    let mut mutations = Vec::new();
    if let Some(enabled) = enabled {
        mutations.push(PlannedMutation {
            resource: control_resource,
            kind: mutation_kind(&control_state),
            expected_digest: control_state.digest(),
            media_type: "application/json".into(),
            content: Some(super::installation_control_content(enabled)),
        });
    } else {
        let package_state =
            observe_target(&ManagedResourceTarget::directory(installed.package_target))?;
        if !matches!(package_state, TargetState::Directory(_)) {
            return Err(storage_conflict(context, resource));
        }
        mutations.push(PlannedMutation {
            resource: resource.clone(),
            kind: MutationKind::Delete,
            expected_digest: package_state.digest(),
            media_type: "application/vnd.ad.directory".into(),
            content: None,
        });
        if !matches!(control_state, TargetState::Missing) {
            mutations.push(PlannedMutation {
                resource: control_resource,
                kind: MutationKind::Delete,
                expected_digest: control_state.digest(),
                media_type: "application/json".into(),
                content: None,
            });
        }
    }
    push_manifest_mutation(
        context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    if config_state.digest().as_ref() != Some(&synthesized.generated_config_digest) {
        mutations.push(PlannedMutation {
            resource: config_resource,
            kind: mutation_kind(&config_state),
            expected_digest: config_state.digest(),
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(synthesized.content)),
        });
    }
    let read_set = base_config
        .as_deref()
        .map(ContentDigest::sha256)
        .filter(|_| inherit_base_config)
        .map(|expected_digest| {
            vec![ReadPrecondition {
                resource: project_resource(context, "base-config"),
                expected_digest,
                write_policy: WritePolicy::ReadOnly,
            }]
        })
        .unwrap_or_default();
    Ok(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    })
}

fn effective_project_runtime(
    requested_context: &AgentContext,
) -> Result<(AgentContext, super::ProjectCodexRuntime), AgentError> {
    let project_path = requested_context.project_path.as_deref().ok_or_else(|| {
        agent_error(
            AgentErrorCode::Unsupported,
            requested_context,
            None,
            "Codex user Plugin installation requires the Agent marketplace flow",
        )
    })?;
    if let Some(runtime) = project_runtime_for_context(requested_context)? {
        return Ok((requested_context.clone(), runtime));
    }
    let runtime = super::project_runtime_descriptor_for_base_project(
        &requested_context.installation_id,
        Path::new(project_path),
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            requested_context,
            None,
            error.to_string(),
        )
    })?
    .ok_or_else(|| {
        agent_error(
            AgentErrorCode::Unsupported,
            requested_context,
            None,
            "Codex project runtime is unavailable",
        )
    })?;
    Ok((
        AgentContext {
            installation_id: runtime.runtime_installation_id.clone(),
            project_path: requested_context.project_path.clone(),
        },
        runtime,
    ))
}

fn desired_inherit_base_config(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
) -> Result<bool, AgentError> {
    if let Some(snapshot) = load_project_codex_runtime_manifest(runtime).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })? {
        return Ok(snapshot.manifest.applied_inherit_base_config);
    }
    let path = projects_state_path()
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(runtime.applied_inherit_base_config)
        }
        Err(error) => {
            return Err(agent_error(
                AgentErrorCode::Io,
                context,
                None,
                error.to_string(),
            ))
        }
    };
    let projects: Vec<crate::models::Project> =
        serde_json::from_slice(&bytes).map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Invalid project registry: {error}"),
            )
        })?;
    Ok(projects
        .into_iter()
        .find(|project| Some(project.path.as_str()) == context.project_path.as_deref())
        .map(|project| project.inherit_base_config)
        .unwrap_or(runtime.applied_inherit_base_config))
}

fn plan_project_override(
    context: &AgentContext,
    resource: &ResourceRef,
    enabled: Option<bool>,
    runtime: &super::ProjectCodexRuntime,
) -> Result<MutationPlan, AgentError> {
    let config_target = ManagedResourceTarget::file(runtime.runtime_home.join("config.toml"));
    let config_state = observe_target(&config_target)?;
    validate_runtime_config_state(context, runtime, resource, &config_state)?;
    let snapshot = load_project_codex_runtime_manifest(runtime).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })?;
    let snapshot = snapshot.ok_or_else(|| {
        agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            Some(resource.clone()),
            "Legacy Project Codex runtime needs Preview and Apply before Plugin changes",
        )
    })?;
    let inherit_base_config = snapshot.manifest.applied_inherit_base_config;
    let profile_id = snapshot.manifest.applied_profile_id;
    let project_settings_keys = snapshot.manifest.project_settings_keys;
    let project_settings =
        project_settings_from_config(context, &config_state, &project_settings_keys)?;
    let mut overlay = snapshot.manifest.project_overlay;
    match enabled {
        Some(enabled) => {
            overlay
                .enabled_plugins
                .insert(resource.logical_id.clone(), enabled);
        }
        None if overlay
            .enabled_plugins
            .remove(&resource.logical_id)
            .is_none() =>
        {
            return Err(agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Project Plugin override is no longer present",
            ));
        }
        None => {}
    }
    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), context, None)?;
    let inherited_config = inherit_base_config
        .then_some(base_config.as_deref())
        .flatten();
    let synthesized = synthesize_project_codex_config_with_settings(
        inherited_config,
        &base_home,
        &overlay,
        &project_settings,
    )
    .map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let manifest = ProjectCodexRuntimeManifest {
        schema_version: PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
        applied_inherit_base_config: inherit_base_config,
        applied_profile_id: profile_id,
        project_overlay: overlay,
        project_settings_keys,
    };
    let manifest_resource = project_resource(context, "runtime-manifest");
    let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
    validate_runtime_manifest_state(context, runtime, &manifest_resource, &manifest_state)?;
    let mut mutations = Vec::new();
    push_manifest_mutation(
        context,
        &mut mutations,
        manifest_resource,
        &manifest_state,
        &manifest,
    )?;
    if config_state.digest().as_ref() != Some(&synthesized.generated_config_digest) {
        mutations.push(PlannedMutation {
            resource: resource.clone(),
            kind: mutation_kind(&config_state),
            expected_digest: config_state.digest(),
            media_type: "application/toml".into(),
            content: Some(serde_json::Value::String(synthesized.content)),
        });
    }
    let read_set = base_config
        .as_deref()
        .map(ContentDigest::sha256)
        .filter(|_| inherit_base_config)
        .map(|expected_digest| {
            vec![ReadPrecondition {
                resource: project_resource(context, "base-config"),
                expected_digest,
                write_policy: WritePolicy::ReadOnly,
            }]
        })
        .unwrap_or_default();
    Ok(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
    })
}

fn validate_user_resource(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<(), AgentError> {
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Plugins
        || resource.scope != ResourceScope::User
        || resource.project_path.is_some()
        || resource.logical_id.is_empty()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Plugin resource does not belong to the active Agent context",
        ));
    }
    Ok(())
}

fn reusable_auth_paths(
    context: &AgentContext,
    auth: SharedAuthBinding,
) -> Result<(PathBuf, PathBuf), AgentError> {
    match auth {
        SharedAuthBinding::FileSymlink { source, target } => Ok((source, target)),
        SharedAuthBinding::KeychainRequiresSharedHome => Err(agent_error(
            AgentErrorCode::Unsupported,
            context,
            None,
            "Keychain-only Codex login cannot be reused by a custom CODEX_HOME; choose the shared Home/Profile route or switch the user credential store to file",
        )),
        SharedAuthBinding::MissingBaseLogin => Err(agent_error(
            AgentErrorCode::Unsupported,
            context,
            None,
            "The base Codex Home has no reusable file login; sign in once with the shared user Home before preparing a Project runtime",
        )),
    }
}

fn validate_runtime_config_state(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    resource: &ResourceRef,
    state: &TargetState,
) -> Result<(), AgentError> {
    if !matches!(state, TargetState::Missing | TargetState::File(_)) {
        return Err(storage_conflict(context, resource));
    }
    if let (Some(recorded), Some(actual)) = (
        runtime.generated_config_digest.as_ref(),
        state.digest().as_ref(),
    ) {
        if recorded != actual {
            return Err(agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Generated Project Codex config was modified outside AD",
            ));
        }
    }
    Ok(())
}

fn validate_runtime_manifest_state(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    resource: &ResourceRef,
    state: &TargetState,
) -> Result<(), AgentError> {
    if !matches!(state, TargetState::Missing | TargetState::File(_)) {
        return Err(storage_conflict(context, resource));
    }
    if let Some(recorded) = runtime.manifest_digest.as_ref() {
        if state.digest().as_ref() != Some(recorded) {
            return Err(agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Project Codex runtime manifest was modified outside AD",
            ));
        }
    }
    Ok(())
}

fn validate_project_resource(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<(), AgentError> {
    if resource.installation_id != context.installation_id
        || resource.kind != ResourceKind::Plugins
        || resource.scope != ResourceScope::Project
        || resource.project_path != context.project_path
        || resource.logical_id.is_empty()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            "Project Plugin resource does not belong to the active Agent context",
        ));
    }
    Ok(())
}

fn validate_project_plugin_resource(
    context: &AgentContext,
    resource: &ResourceRef,
) -> Result<(), AgentError> {
    validate_project_resource(context, resource)?;
    validate_plugin_id(context, &resource.logical_id)
}

fn resolve_project_resource(
    context: &AgentContext,
    resource: &ResourceRef,
    runtime: &super::ProjectCodexRuntime,
) -> Result<ManagedResourceTarget, AgentError> {
    validate_project_resource(context, resource)?;
    let logical_id = resource.logical_id.as_str();
    if logical_id == "runtime-auth" {
        return Ok(ManagedResourceTarget::symlink(
            runtime.runtime_home.join("auth.json"),
        ));
    }
    if logical_id == "base-config" {
        return Ok(ManagedResourceTarget::file(
            base_home(context, &runtime.base_installation_id)?.join("config.toml"),
        ));
    }
    if logical_id == "runtime-config" {
        return Ok(ManagedResourceTarget::file(
            runtime.runtime_home.join("config.toml"),
        ));
    }
    if logical_id == "runtime-manifest" {
        return Ok(ManagedResourceTarget::file(runtime.manifest_path()));
    }
    if logical_id == "project-policy" {
        return projects_state_path()
            .map(ManagedResourceTarget::file)
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    Some(resource.clone()),
                    error.to_string(),
                )
            });
    }
    if managed_catalog_identity(logical_id).is_some() {
        return managed_codex_package_target(context, resource, runtime)
            .map(ManagedResourceTarget::directory);
    }
    if let Some(id) = logical_id.strip_prefix("plugin-control:") {
        return super::installation_control_path(&super::ResourceInstallationId::from(
            id.to_owned(),
        ))
        .map(ManagedResourceTarget::file)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::Io,
                context,
                Some(resource.clone()),
                error.to_string(),
            )
        });
    }
    if let Some(name) = logical_id.strip_prefix("marketplace:") {
        validate_segment(context, "marketplace", name)?;
        return Ok(ManagedResourceTarget::directory(
            runtime.runtime_home.join(".tmp/marketplaces").join(name),
        ));
    }
    if let Some(segments) = logical_id.strip_prefix("package:") {
        let segments = segments.split(':').collect::<Vec<_>>();
        if segments.len() != 3 {
            return Err(unknown_project_resource(context, resource));
        }
        validate_segment(context, "marketplace", segments[0])?;
        validate_segment(context, "plugin", segments[1])?;
        validate_segment(context, "version", segments[2])?;
        return Ok(ManagedResourceTarget::directory(
            runtime
                .runtime_home
                .join("plugins/cache")
                .join(segments[0])
                .join(segments[1])
                .join(segments[2]),
        ));
    }
    validate_plugin_id(context, logical_id)?;
    Ok(ManagedResourceTarget::file(
        runtime.runtime_home.join("config.toml"),
    ))
}

fn managed_catalog_identity(logical_id: &str) -> Option<(&str, &str)> {
    let (source_id, install_id) = logical_id.rsplit_once('/')?;
    source_id
        .starts_with("skill-source:")
        .then_some((source_id, install_id))
}

fn managed_codex_package_target(
    context: &AgentContext,
    resource: &ResourceRef,
    runtime: &super::ProjectCodexRuntime,
) -> Result<PathBuf, AgentError> {
    let (_, install_id) = managed_catalog_identity(&resource.logical_id)
        .ok_or_else(|| unknown_project_resource(context, resource))?;
    let state = super::execution_state::ExecutionState::open()
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
    if let Some(record) = super::load_ownership_record(&state, resource)? {
        if record.target_kind != ResourceStateKind::Directory {
            return Err(agent_error(
                AgentErrorCode::PermissionDenied,
                context,
                Some(resource.clone()),
                "Managed Codex Plugin ownership target is not a directory",
            ));
        }
        let target = PathBuf::from(record.target_path);
        validate_managed_package_target(context, resource, runtime, &target, install_id)?;
        return Ok(target);
    }

    let (source_id, _) = managed_catalog_identity(&resource.logical_id).unwrap();
    let catalog = super::load_resource_catalog_snapshot().map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let candidate = catalog
        .resources
        .values()
        .find(|candidate| {
            candidate.source_id == source_id
                && candidate.install_id == install_id
                && candidate.kind == ResourceKind::Plugins
                && candidate.present
                && candidate.lifecycle == super::ResourceLifecycle::Managed
        })
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                Some(resource.clone()),
                "Managed Codex Plugin catalog resource is unavailable",
            )
        })?;
    let resolved = super::resolve_catalog_resource(&candidate.id).map_err(|error| {
        agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let metadata = read_codex_catalog_plugin_metadata(&resolved.physical_path, install_id)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::Unsupported,
                context,
                Some(resource.clone()),
                error,
            )
        })?;
    let target = runtime
        .runtime_home
        .join("plugins/cache")
        .join(metadata.marketplace_name)
        .join(metadata.plugin_name)
        .join(metadata.version);
    validate_managed_package_target(context, resource, runtime, &target, install_id)?;
    Ok(target)
}

fn validate_managed_package_target(
    context: &AgentContext,
    resource: &ResourceRef,
    runtime: &super::ProjectCodexRuntime,
    target: &Path,
    install_id: &str,
) -> Result<(), AgentError> {
    let cache = runtime.runtime_home.join("plugins/cache");
    let relative = target.strip_prefix(&cache).map_err(|_| {
        agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            Some(resource.clone()),
            "Managed Codex Plugin target escapes the project runtime cache",
        )
    })?;
    let components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() != 3
        || components[1] != install_id
        || components.iter().any(|segment| !valid_segment(segment))
    {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            Some(resource.clone()),
            "Managed Codex Plugin target identity is invalid",
        ));
    }
    Ok(())
}

fn unknown_project_resource(context: &AgentContext, resource: &ResourceRef) -> AgentError {
    agent_error(
        AgentErrorCode::InvalidPlan,
        context,
        Some(resource.clone()),
        "Unknown Project Codex Plugin resource",
    )
}

fn base_home(
    context: &AgentContext,
    installation_id: &super::InstallationId,
) -> Result<PathBuf, AgentError> {
    discover_codex_candidates()
        .into_iter()
        .find(|candidate| candidate.installation().id == *installation_id)
        .map(|candidate| PathBuf::from(&candidate.installation().root_path))
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Project runtime base Codex installation is no longer available",
            )
        })
}

fn parse_credential_store(
    context: &AgentContext,
    bytes: &[u8],
) -> Result<Option<String>, AgentError> {
    let config = std::str::from_utf8(bytes)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?;
    Ok(config
        .get("cli_auth_credentials_store")
        .and_then(toml::Value::as_str)
        .map(str::to_owned))
}

fn project_overlays_for_plan(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    _state: &TargetState,
    _inherit_base_config: bool,
) -> Result<(ProjectPluginOverlay, BTreeSet<String>), AgentError> {
    if let Some(snapshot) = load_project_codex_runtime_manifest(runtime).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            error.to_string(),
        )
    })? {
        return Ok((
            snapshot.manifest.project_overlay,
            snapshot.manifest.project_settings_keys,
        ));
    }
    Ok((ProjectPluginOverlay::default(), BTreeSet::new()))
}

fn project_settings_from_config(
    context: &AgentContext,
    state: &TargetState,
    keys: &BTreeSet<String>,
) -> Result<BTreeMap<String, toml::Value>, AgentError> {
    if keys.is_empty() {
        return Ok(BTreeMap::new());
    }
    let TargetState::File(bytes) = state else {
        return Err(agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            None,
            "Project settings provenance exists but the generated config is missing",
        ));
    };
    let config = std::str::from_utf8(bytes)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?
        .parse::<toml::Value>()
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?;
    let root = config.as_table().ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Project Codex config must be a TOML table",
        )
    })?;
    keys.iter()
        .map(|key| {
            root.get(key)
                .cloned()
                .map(|value| (key.clone(), value))
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::ResourceChanged,
                        context,
                        None,
                        format!("Generated Project setting {key} is missing"),
                    )
                })
        })
        .collect()
}

fn validate_plugin_id(context: &AgentContext, plugin_id: &str) -> Result<(), AgentError> {
    let segments = plugin_id.split('@').collect::<Vec<_>>();
    if segments.len() != 2 {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Plugin id must use <plugin>@<marketplace>",
        ));
    }
    validate_segment(context, "plugin", segments[0])?;
    validate_segment(context, "marketplace", segments[1])
}

fn validate_segment(context: &AgentContext, kind: &str, value: &str) -> Result<(), AgentError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid {kind} path segment: {value}"),
        ));
    }
    Ok(())
}

fn project_resource(context: &AgentContext, logical_id: impl Into<String>) -> ResourceRef {
    ResourceRef {
        installation_id: context.installation_id.clone(),
        project_path: context.project_path.clone(),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: logical_id.into(),
    }
}

fn mutation_kind(state: &TargetState) -> MutationKind {
    if matches!(state, TargetState::Missing) {
        MutationKind::Create
    } else {
        MutationKind::Replace
    }
}

fn push_symlink_mutation(
    mutations: &mut Vec<PlannedMutation>,
    resource: ResourceRef,
    state: &TargetState,
    source: &Path,
) {
    if matches!(state, TargetState::Symlink(existing) if existing == source) {
        return;
    }
    mutations.push(PlannedMutation {
        resource,
        kind: mutation_kind(state),
        expected_digest: state.digest(),
        media_type: "application/vnd.ad.symlink".into(),
        content: Some(serde_json::Value::String(
            source.to_string_lossy().into_owned(),
        )),
    });
}

fn push_manifest_mutation(
    context: &AgentContext,
    mutations: &mut Vec<PlannedMutation>,
    resource: ResourceRef,
    state: &TargetState,
    manifest: &ProjectCodexRuntimeManifest,
) -> Result<(), AgentError> {
    let validated = render_project_codex_runtime_manifest(manifest).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let content = serde_json::from_slice::<serde_json::Value>(&validated).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    let rendered = serde_json::to_vec_pretty(&content).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            Some(resource.clone()),
            error.to_string(),
        )
    })?;
    if state.digest().as_ref() == Some(&ContentDigest::sha256(&rendered)) {
        return Ok(());
    }
    mutations.push(PlannedMutation {
        resource,
        kind: mutation_kind(state),
        expected_digest: state.digest(),
        media_type: "application/json".into(),
        content: Some(content),
    });
    Ok(())
}

fn push_directory_mutation(
    mutations: &mut Vec<PlannedMutation>,
    resource: ResourceRef,
    state: &TargetState,
    source: &Path,
    digest: ContentDigest,
) {
    mutations.push(PlannedMutation {
        resource,
        kind: mutation_kind(state),
        expected_digest: state.digest(),
        media_type: "application/vnd.ad.directory".into(),
        content: Some(serde_json::json!({
            "path": source.to_string_lossy(),
            "digest": digest,
        })),
    });
}

fn push_catalog_plugin_directory_mutation(
    mutations: &mut Vec<PlannedMutation>,
    resource: ResourceRef,
    state: &TargetState,
    source: &Path,
    digest: ContentDigest,
) {
    mutations.push(PlannedMutation {
        resource,
        kind: mutation_kind(state),
        expected_digest: state.digest(),
        media_type: "application/vnd.ad.directory".into(),
        content: Some(serde_json::json!({
            "path": source.to_string_lossy(),
            "digest": digest,
            "excludeAgentSkillProjections": true,
        })),
    });
}

fn catalog_plugin_tree_digest(source: &Path) -> Result<ContentDigest, std::io::Error> {
    super::execution_fs::directory_tree_digest_filtered(source, |path| {
        Ok(!super::resource_scanner::is_agent_skill_projection(path))
    })
}

fn storage_conflict(context: &AgentContext, resource: &ResourceRef) -> AgentError {
    agent_error(
        AgentErrorCode::ResourceChanged,
        context,
        Some(resource.clone()),
        "Project Plugin target changed storage type",
    )
}

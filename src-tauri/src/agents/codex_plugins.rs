use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use semver::Version;
use serde::Deserialize;

use crate::fs::paths::{ad_home, projects_state_path};

use super::codex::discover_codex_candidates;
use super::codex_ports::{
    agent_error, project_runtime_for_context, read_optional, resolve_codex_home,
};
use super::execution_fs::{directory_tree_digest, observe_target, TargetState};
use super::{
    load_project_codex_runtime_manifest, render_project_codex_runtime_manifest,
    synthesize_project_codex_config, synthesize_project_codex_config_with_settings, AgentContext,
    AgentError, AgentErrorCode, AgentId, CapabilityAvailability, CapabilityLimitation,
    CapabilityOperation, CollectionInstallRequest, ContentDigest, ManagedResourceTarget,
    MarketplaceOverlay, MutationKind, MutationPlan, PlanId, PlannedMutation, PluginInstallProgress,
    PluginInstallProgressReporter, PluginsPort, ProjectCodexRuntimeManifest, ProjectPluginOverlay,
    ReadPrecondition, ResourceKind, ResourceLocation, ResourceOrigin, ResourcePort, ResourceRef,
    ResourceScope, ResourceSnapshot, SettingsEdit, SharedAuthBinding, WritePolicy,
    PROJECT_CODEX_RUNTIME_MANIFEST_SCHEMA_VERSION,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectPluginInstallSource {
    marketplace: ProjectMarketplaceSource,
    package: ProjectPackageSource,
    #[serde(default = "default_true")]
    inherit_base_config: bool,
    #[serde(default)]
    profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectMarketplaceSource {
    name: String,
    source_type: String,
    source: String,
    #[serde(default)]
    ref_name: Option<String>,
    #[serde(default)]
    last_revision: Option<String>,
    stage_path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectPackageSource {
    name: String,
    version: String,
    stage_path: PathBuf,
}

struct InheritedPluginSnapshot {
    marketplace: String,
    plugin: String,
    version: String,
    marketplace_source: PathBuf,
    marketplace_digest: ContentDigest,
    package_source: PathBuf,
    package_digest: ContentDigest,
}

#[derive(Debug, Default)]
pub(crate) struct CodexPluginsPort;

pub(super) fn plan_project_runtime_bootstrap(
    context: &AgentContext,
    inherit_base_config: bool,
    profile_id: Option<&str>,
    report: &PluginInstallProgressReporter<'_>,
) -> Result<Option<MutationPlan>, AgentError> {
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
    let inherited = if inherit_base_config {
        prepare_inherited_plugins(context, &base_home, base_config.as_deref(), "", report)?
    } else {
        Vec::new()
    };

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
    append_inherited_marketplace_mutations(context, &runtime, "", &inherited, &mut mutations)?;
    append_inherited_package_mutations(context, &runtime, &inherited, &mut mutations)?;
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

    Ok(Some(MutationPlan {
        id: PlanId::from(uuid::Uuid::new_v4().to_string()),
        agent_id: AgentId::from("codex"),
        context: context.clone(),
        read_set,
        mutations,
        expires_at: Utc::now() + Duration::minutes(5),
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
        self.plan_install_with_progress(context, request, &|_| {})
    }

    fn plan_install_with_progress(
        &self,
        context: &AgentContext,
        request: CollectionInstallRequest,
        report: &PluginInstallProgressReporter<'_>,
    ) -> Result<MutationPlan, AgentError> {
        let Some(runtime) = project_runtime_for_context(context)? else {
            return Err(agent_error(
                AgentErrorCode::Unsupported,
                context,
                None,
                "Codex user plugin installation requires its marketplace and authorization flow",
            ));
        };
        let source: ProjectPluginInstallSource =
            serde_json::from_value(request.source).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Invalid Project Plugin install source: {error}"),
                )
            })?;
        validate_install_source(context, &request.logical_id, &source)?;

        let base_home = base_home(context, &runtime.base_installation_id)?;
        let base_config_path = base_home.join("config.toml");
        let base_config = read_optional(&base_config_path, context, None)?;
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

        let marketplace_digest =
            directory_tree_digest(&source.marketplace.stage_path).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Invalid staged marketplace: {error}"),
                )
            })?;
        let package_digest =
            directory_tree_digest(&source.package.stage_path).map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Invalid staged Plugin package: {error}"),
                )
            })?;
        validate_staged_manifests(context, &source)?;
        let inherited = if source.inherit_base_config {
            prepare_inherited_plugins(
                context,
                &base_home,
                base_config.as_deref(),
                &request.logical_id,
                report,
            )?
        } else {
            Vec::new()
        };

        let config_resource = project_resource(context, request.logical_id.clone());
        let config_target = ManagedResourceTarget::file(runtime.runtime_home.join("config.toml"));
        let config_state = observe_target(&config_target)?;
        validate_runtime_config_state(context, &runtime, &config_resource, &config_state)?;

        let marketplace_overlay = MarketplaceOverlay {
            source_type: source.marketplace.source_type.clone(),
            source: source.marketplace.source.clone(),
            ref_name: source.marketplace.ref_name.clone(),
            last_revision: source.marketplace.last_revision.clone(),
        };
        let (mut overlay, project_settings_keys) = project_overlays_for_plan(
            context,
            &runtime,
            &config_state,
            source.inherit_base_config,
        )?;
        let project_settings =
            project_settings_from_config(context, &config_state, &project_settings_keys)?;
        if overlay
            .marketplaces
            .get(&source.marketplace.name)
            .is_some_and(|existing| existing != &marketplace_overlay)
        {
            return Err(agent_error(
                AgentErrorCode::ResourceChanged,
                context,
                None,
                format!(
                    "Project marketplace {} already uses a different source",
                    source.marketplace.name
                ),
            ));
        }
        overlay
            .marketplaces
            .insert(source.marketplace.name.clone(), marketplace_overlay);
        overlay
            .enabled_plugins
            .insert(request.logical_id.clone(), true);
        let inherited_config = source
            .inherit_base_config
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
            applied_inherit_base_config: source.inherit_base_config,
            applied_profile_id: source.profile_id.clone(),
            project_overlay: overlay,
            project_settings_keys,
        };

        let auth_resource = project_resource(context, "runtime-auth");
        let manifest_resource = project_resource(context, "runtime-manifest");
        let marketplace_resource =
            project_resource(context, format!("marketplace:{}", source.marketplace.name));
        let package_resource = project_resource(
            context,
            format!(
                "package:{}:{}:{}",
                source.marketplace.name, source.package.name, source.package.version
            ),
        );

        let auth_state = observe_target(&ManagedResourceTarget::symlink(auth_target))?;
        if !matches!(auth_state, TargetState::Missing | TargetState::Symlink(_)) {
            return Err(storage_conflict(context, &auth_resource));
        }
        let manifest_state = observe_target(&ManagedResourceTarget::file(runtime.manifest_path()))?;
        validate_runtime_manifest_state(context, &runtime, &manifest_resource, &manifest_state)?;
        let marketplace_target =
            resolve_project_resource(context, &marketplace_resource, &runtime)?;
        let marketplace_state = observe_target(&marketplace_target)?;
        if !matches!(
            marketplace_state,
            TargetState::Missing | TargetState::Directory(_)
        ) {
            return Err(storage_conflict(context, &marketplace_resource));
        }
        let package_target = resolve_project_resource(context, &package_resource, &runtime)?;
        let package_state = observe_target(&package_target)?;
        if !matches!(
            package_state,
            TargetState::Missing | TargetState::Directory(_)
        ) {
            return Err(storage_conflict(context, &package_resource));
        }
        if let TargetState::Directory(existing) = &package_state {
            if existing != &package_digest {
                return Err(agent_error(
                    AgentErrorCode::ResourceChanged,
                    context,
                    Some(package_resource),
                    "The same Project Plugin version already exists with different content",
                ));
            }
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
        append_inherited_marketplace_mutations(
            context,
            &runtime,
            &source.marketplace.name,
            &inherited,
            &mut mutations,
        )?;
        push_directory_mutation(
            &mut mutations,
            marketplace_resource,
            &marketplace_state,
            &source.marketplace.stage_path,
            marketplace_digest,
        );
        append_inherited_package_mutations(context, &runtime, &inherited, &mut mutations)?;
        if package_state.digest().as_ref() != Some(&package_digest) {
            push_directory_mutation(
                &mut mutations,
                package_resource,
                &package_state,
                &source.package.stage_path,
                package_digest,
            );
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

        Ok(MutationPlan {
            id: PlanId::from(uuid::Uuid::new_v4().to_string()),
            agent_id: AgentId::from("codex"),
            context: context.clone(),
            read_set,
            mutations,
            expires_at: Utc::now() + Duration::minutes(5),
        })
    }

    fn plan_set_enabled(
        &self,
        context: &AgentContext,
        resource: &ResourceRef,
        enabled: bool,
    ) -> Result<MutationPlan, AgentError> {
        if let Some(runtime) = project_runtime_for_context(context)? {
            validate_project_plugin_resource(context, resource)?;
            return plan_project_set_enabled(context, resource, enabled, &runtime);
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
}

fn plan_project_set_enabled(
    context: &AgentContext,
    resource: &ResourceRef,
    enabled: bool,
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
    overlay
        .enabled_plugins
        .insert(resource.logical_id.clone(), enabled);
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

fn validate_install_source(
    context: &AgentContext,
    plugin_id: &str,
    source: &ProjectPluginInstallSource,
) -> Result<(), AgentError> {
    validate_plugin_id(context, plugin_id)?;
    validate_segment(context, "marketplace", &source.marketplace.name)?;
    validate_segment(context, "plugin", &source.package.name)?;
    validate_segment(context, "version", &source.package.version)?;
    if plugin_id != format!("{}@{}", source.package.name, source.marketplace.name) {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Plugin id does not match the staged package and marketplace",
        ));
    }
    if !matches!(source.marketplace.source_type.as_str(), "git" | "local")
        || source.marketplace.source.trim().is_empty()
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Marketplace source must be a non-empty git or local source",
        ));
    }
    validate_stage_path(context, &source.marketplace.stage_path)?;
    validate_stage_path(context, &source.package.stage_path)?;
    Ok(())
}

fn validate_stage_path(context: &AgentContext, path: &Path) -> Result<(), AgentError> {
    let managed_root = ad_home()
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?
        .join("staging/codex-plugin-conversion");
    if !path.is_absolute() || !path.starts_with(&managed_root) {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            None,
            "Plugin install sources must be physical directories in AD-owned conversion staging",
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid staged directory {}: {error}", path.display()),
        )
    })?;
    let managed_root = std::fs::canonicalize(&managed_root)
        .map_err(|error| agent_error(AgentErrorCode::Io, context, None, error.to_string()))?;
    if !canonical.is_dir() || !canonical.starts_with(&managed_root) {
        return Err(agent_error(
            AgentErrorCode::PermissionDenied,
            context,
            None,
            "Plugin install sources must be physical directories in AD-owned conversion staging",
        ));
    }
    Ok(())
}

fn prepare_inherited_plugins(
    context: &AgentContext,
    base_home: &Path,
    base_config: Option<&[u8]>,
    project_plugin_id: &str,
    report: &PluginInstallProgressReporter<'_>,
) -> Result<Vec<InheritedPluginSnapshot>, AgentError> {
    let Some(bytes) = base_config else {
        return Ok(Vec::new());
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
    let Some(plugins) = config.get("plugins").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let mut enabled_plugins = plugins
        .iter()
        .filter(|(plugin_id, plugin_config)| {
            plugin_id.as_str() != project_plugin_id
                && plugin_config.get("enabled").and_then(toml::Value::as_bool) != Some(false)
        })
        .collect::<Vec<_>>();
    enabled_plugins.sort_by(|left, right| left.0.cmp(right.0));
    let total = enabled_plugins.len();
    let mut inherited = Vec::new();
    let mut marketplace_snapshots = BTreeMap::<String, (PathBuf, ContentDigest)>::new();
    for (index, (plugin_id, _plugin_config)) in enabled_plugins.into_iter().enumerate() {
        report(PluginInstallProgress {
            logical_id: plugin_id.clone(),
            current: index + 1,
            total,
        });
        validate_plugin_id(context, plugin_id)?;
        let (plugin, marketplace) = plugin_id.split_once('@').ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Invalid inherited Plugin id: {plugin_id}"),
            )
        })?;
        let (marketplace_source, marketplace_digest) = if let Some(snapshot) =
            marketplace_snapshots.get(marketplace)
        {
            snapshot.clone()
        } else {
            let source =
                inherited_marketplace_source(context, base_home, &config, marketplace, plugin_id)?;
            if !source.join(".agents/plugins/marketplace.json").is_file()
                && !source.join(".claude-plugin/marketplace.json").is_file()
            {
                return Err(agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!(
                        "Enabled base Plugin {plugin_id} has no verifiable marketplace snapshot"
                    ),
                ));
            }
            let snapshot = snapshot_inherited_directory(context, &source)?;
            marketplace_snapshots.insert(marketplace.to_string(), snapshot.clone());
            snapshot
        };
        let package_base = base_home
            .join("plugins/cache")
            .join(marketplace)
            .join(plugin);
        let version = active_plugin_version(&package_base).ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Enabled base Plugin {plugin_id} has no installed package cache"),
            )
        })?;
        let package_source = package_base.join(&version);
        validate_inherited_manifest(context, &package_source, plugin, &version)?;
        let (package_source, package_digest) =
            snapshot_inherited_directory(context, &package_source)?;
        inherited.push(InheritedPluginSnapshot {
            marketplace: marketplace.to_string(),
            plugin: plugin.to_string(),
            version,
            marketplace_source,
            marketplace_digest,
            package_source,
            package_digest,
        });
    }
    inherited.sort_by(|left, right| {
        (&left.marketplace, &left.plugin).cmp(&(&right.marketplace, &right.plugin))
    });
    Ok(inherited)
}

fn inherited_marketplace_source(
    context: &AgentContext,
    base_home: &Path,
    config: &toml::Value,
    marketplace: &str,
    plugin_id: &str,
) -> Result<PathBuf, AgentError> {
    let marketplace_config = config
        .get("marketplaces")
        .and_then(toml::Value::as_table)
        .and_then(|marketplaces| marketplaces.get(marketplace))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Enabled base Plugin {plugin_id} has no marketplace configuration"),
            )
        })?;
    let source_type = marketplace_config
        .get("source_type")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!("Enabled base Plugin {plugin_id} has no marketplace source type"),
            )
        })?;
    let source = match source_type {
        "git" => base_home.join(".tmp/marketplaces").join(marketplace),
        "local" => {
            let configured = marketplace_config
                .get("source")
                .and_then(toml::Value::as_str)
                .filter(|source| !source.is_empty())
                .ok_or_else(|| {
                    agent_error(
                        AgentErrorCode::InvalidPlan,
                        context,
                        None,
                        format!("Enabled base Plugin {plugin_id} has no local marketplace source"),
                    )
                })?;
            let configured = PathBuf::from(configured);
            if configured.is_absolute() {
                configured
            } else {
                base_home.join(configured)
            }
        }
        other => {
            return Err(agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                format!(
                "Enabled base Plugin {plugin_id} uses unsupported marketplace source type {other}"
            ),
            ))
        }
    };
    std::fs::canonicalize(&source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!(
                "Enabled base Plugin {plugin_id} marketplace source {} is unavailable: {error}",
                source.display()
            ),
        )
    })
}

fn active_plugin_version(package_base: &Path) -> Option<String> {
    let mut versions = std::fs::read_dir(package_base)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            entry.file_type().ok().filter(std::fs::FileType::is_dir)?;
            entry.file_name().into_string().ok()
        })
        .filter(|version| {
            !version.is_empty()
                && version != "."
                && version != ".."
                && version.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        })
        .collect::<Vec<_>>();
    if versions.iter().any(|version| version == "local") {
        return Some("local".into());
    }
    versions.sort_by(
        |left, right| match (Version::parse(left), Version::parse(right)) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            _ => left.cmp(right),
        },
    );
    versions.pop()
}

fn validate_inherited_manifest(
    context: &AgentContext,
    package: &Path,
    plugin: &str,
    version: &str,
) -> Result<(), AgentError> {
    let manifest = parse_json_file(
        context,
        &package.join(".codex-plugin/plugin.json"),
        "inherited Codex Plugin manifest",
    )?;
    if manifest.get("name").and_then(serde_json::Value::as_str) != Some(plugin)
        || version != "local"
            && manifest.get("version").and_then(serde_json::Value::as_str) != Some(version)
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Inherited Plugin manifest does not match {plugin}@{version}"),
        ));
    }
    Ok(())
}

fn snapshot_inherited_directory(
    context: &AgentContext,
    source: &Path,
) -> Result<(PathBuf, ContentDigest), AgentError> {
    let source = std::fs::canonicalize(source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Inherited Plugin source is unavailable: {error}"),
        )
    })?;
    let digest = directory_tree_digest(&source).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid inherited Plugin directory: {error}"),
        )
    })?;
    Ok((source, digest))
}

fn append_inherited_marketplace_mutations(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    project_marketplace: &str,
    inherited: &[InheritedPluginSnapshot],
    mutations: &mut Vec<PlannedMutation>,
) -> Result<(), AgentError> {
    let mut seen = BTreeSet::new();
    for snapshot in inherited {
        if snapshot.marketplace == project_marketplace || !seen.insert(snapshot.marketplace.clone())
        {
            continue;
        }
        let resource = project_resource(context, format!("marketplace:{}", snapshot.marketplace));
        let state = observe_target(&resolve_project_resource(context, &resource, runtime)?)?;
        if !matches!(state, TargetState::Missing | TargetState::Directory(_)) {
            return Err(storage_conflict(context, &resource));
        }
        push_directory_mutation(
            mutations,
            resource,
            &state,
            &snapshot.marketplace_source,
            snapshot.marketplace_digest.clone(),
        );
    }
    Ok(())
}

fn append_inherited_package_mutations(
    context: &AgentContext,
    runtime: &super::ProjectCodexRuntime,
    inherited: &[InheritedPluginSnapshot],
    mutations: &mut Vec<PlannedMutation>,
) -> Result<(), AgentError> {
    for snapshot in inherited {
        let resource = project_resource(
            context,
            format!(
                "package:{}:{}:{}",
                snapshot.marketplace, snapshot.plugin, snapshot.version
            ),
        );
        let state = observe_target(&resolve_project_resource(context, &resource, runtime)?)?;
        if !matches!(state, TargetState::Missing | TargetState::Directory(_)) {
            return Err(storage_conflict(context, &resource));
        }
        push_directory_mutation(
            mutations,
            resource,
            &state,
            &snapshot.package_source,
            snapshot.package_digest.clone(),
        );
    }
    Ok(())
}

fn validate_staged_manifests(
    context: &AgentContext,
    source: &ProjectPluginInstallSource,
) -> Result<(), AgentError> {
    let marketplace_manifest = [
        source
            .marketplace
            .stage_path
            .join(".agents/plugins/marketplace.json"),
        source
            .marketplace
            .stage_path
            .join(".claude-plugin/marketplace.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .ok_or_else(|| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Staged marketplace has no supported catalog manifest",
        )
    })?;
    parse_json_file(context, &marketplace_manifest, "marketplace catalog")?;

    let plugin_manifest = source.package.stage_path.join(".codex-plugin/plugin.json");
    let manifest = parse_json_file(context, &plugin_manifest, "Codex Plugin manifest")?;
    if manifest.get("name").and_then(serde_json::Value::as_str)
        != Some(source.package.name.as_str())
        || manifest.get("version").and_then(serde_json::Value::as_str)
            != Some(source.package.version.as_str())
    {
        return Err(agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            "Codex Plugin manifest name/version does not match the install target",
        ));
    }
    Ok(())
}

fn parse_json_file(
    context: &AgentContext,
    path: &Path,
    kind: &str,
) -> Result<serde_json::Value, AgentError> {
    let bytes = std::fs::read(path).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Failed to read {kind} {}: {error}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        agent_error(
            AgentErrorCode::InvalidPlan,
            context,
            None,
            format!("Invalid {kind} {}: {error}", path.display()),
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

pub(super) fn validate_legacy_project_plugin_ownership(
    context: &AgentContext,
    explicit_plugin_ids: &BTreeSet<String>,
    inherit_base_config: bool,
) -> Result<bool, AgentError> {
    let Some(runtime) = project_runtime_for_context(context)? else {
        return Ok(false);
    };
    if load_project_codex_runtime_manifest(&runtime)
        .map_err(|error| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                error.to_string(),
            )
        })?
        .is_some()
    {
        return Ok(false);
    }
    let config_state = observe_target(&ManagedResourceTarget::file(
        runtime.runtime_home.join("config.toml"),
    ))?;
    let legacy = project_overlay_from_legacy_config(context, &config_state)?;
    if legacy.marketplaces.is_empty() && legacy.enabled_plugins.is_empty() {
        return Ok(true);
    }

    let base_home = base_home(context, &runtime.base_installation_id)?;
    let base_config = read_optional(&base_home.join("config.toml"), context, None)?;
    let inherited = match base_config.as_deref() {
        Some(base_config) => {
            let synthesized = synthesize_project_codex_config(
                Some(base_config),
                &base_home,
                &ProjectPluginOverlay::default(),
            )
            .map_err(|error| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    error.to_string(),
                )
            })?;
            project_overlay_from_legacy_config(
                context,
                &TargetState::File(synthesized.content.into_bytes()),
            )?
        }
        None => ProjectPluginOverlay::default(),
    };
    let explicit_marketplaces = explicit_plugin_ids
        .iter()
        .filter_map(|plugin_id| {
            plugin_id
                .split_once('@')
                .map(|(_, marketplace)| marketplace)
        })
        .collect::<BTreeSet<_>>();
    let enabled_legacy_marketplaces = legacy
        .enabled_plugins
        .iter()
        .filter(|(_, enabled)| **enabled)
        .filter_map(|(plugin_id, _)| {
            plugin_id
                .split_once('@')
                .map(|(_, marketplace)| marketplace)
        })
        .collect::<BTreeSet<_>>();

    for (plugin_id, enabled) in &legacy.enabled_plugins {
        if !enabled {
            continue;
        }
        if explicit_plugin_ids.contains(plugin_id)
            || inherited.enabled_plugins.get(plugin_id) == Some(enabled)
        {
            continue;
        }
        return Err(agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            None,
            format!(
                "Legacy Project Plugin {plugin_id} has ambiguous ownership; select it explicitly and Preview again"
            ),
        ));
    }
    for (name, marketplace) in &legacy.marketplaces {
        if !enabled_legacy_marketplaces.contains(name.as_str()) {
            continue;
        }
        if explicit_marketplaces.contains(name.as_str())
            || inherited.marketplaces.get(name).is_some_and(|inherited| {
                inherited == marketplace
                    || inherit_base_config && marketplace_ownership_matches(inherited, marketplace)
            })
        {
            continue;
        }
        return Err(agent_error(
            AgentErrorCode::ResourceChanged,
            context,
            None,
            format!(
                "Legacy Project marketplace {name} has ambiguous ownership; select its Plugin explicitly and Preview again"
            ),
        ));
    }
    Ok(true)
}

fn marketplace_ownership_matches(left: &MarketplaceOverlay, right: &MarketplaceOverlay) -> bool {
    left.source_type == right.source_type
        && left.source == right.source
        && left.ref_name == right.ref_name
}

fn project_overlay_from_legacy_config(
    context: &AgentContext,
    state: &TargetState,
) -> Result<ProjectPluginOverlay, AgentError> {
    let TargetState::File(bytes) = state else {
        return Ok(ProjectPluginOverlay::default());
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
    let mut overlay = ProjectPluginOverlay::default();
    if let Some(marketplaces) = config.get("marketplaces").and_then(toml::Value::as_table) {
        for (name, value) in marketplaces {
            let marketplace = value.as_table().ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Project marketplace {name} must be a table"),
                )
            })?;
            let required = |key: &str| {
                marketplace
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        agent_error(
                            AgentErrorCode::InvalidPlan,
                            context,
                            None,
                            format!("Project marketplace {name} has no {key}"),
                        )
                    })
            };
            overlay.marketplaces.insert(
                name.clone(),
                MarketplaceOverlay {
                    source_type: required("source_type")?,
                    source: required("source")?,
                    ref_name: marketplace
                        .get("ref")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned),
                    last_revision: marketplace
                        .get("last_revision")
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned),
                },
            );
        }
    }
    if let Some(plugins) = config.get("plugins").and_then(toml::Value::as_table) {
        for (plugin_id, value) in plugins {
            let plugin = value.as_table().ok_or_else(|| {
                agent_error(
                    AgentErrorCode::InvalidPlan,
                    context,
                    None,
                    format!("Project Plugin {plugin_id} must be a table"),
                )
            })?;
            overlay.enabled_plugins.insert(
                plugin_id.clone(),
                plugin
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
            );
        }
    }
    Ok(overlay)
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
    if state.digest().as_ref() == Some(&digest) {
        return;
    }
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

fn storage_conflict(context: &AgentContext, resource: &ResourceRef) -> AgentError {
    agent_error(
        AgentErrorCode::ResourceChanged,
        context,
        Some(resource.clone()),
        "Project Plugin target changed storage type",
    )
}

fn default_true() -> bool {
    true
}

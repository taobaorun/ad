use std::collections::BTreeMap;

use serde_json::Value;

use super::collection_management::{resource_management, CollectionManagementInput};
pub(super) use super::collection_skills::inspect_skills;
use super::settings_inventory::SettingsLayerSemantic;
use super::{
    builtin_registry, opaque_contract_id, AgentContext, AgentError, AgentErrorCode,
    CategoryCoverage, CollectionResourceInventory, CollectionResourceView, CoverageStatus,
    DeclarationKey, EffectiveResourceState, ItemDiagnostic, PhysicalTargetId,
    ProjectCodexRuntimeManifest, ResourceDeclarationView, ResourceHealthStatus, ResourceHealthView,
    ResourceKey, ResourceKind, ResourceLayer, ResourceOwnershipKind, ResourceOwnershipRecord,
    ResourceOwnershipView, ResourceProvenanceView, ResourceRef, ResourceScope, ResourceSourceKind,
    ResourceSourceView, WorkspaceDescriptor, WorkspaceKey,
};

pub(super) fn inspect_plugins(
    workspace: &WorkspaceDescriptor,
    settings_layers: &[SettingsLayerSemantic],
    runtime_manifest: Option<&ProjectCodexRuntimeManifest>,
    version_diagnostic: &ItemDiagnostic,
) -> Result<CollectionResourceInventory, AgentError> {
    let mut observations = Vec::new();
    for layer in settings_layers {
        let Some(plugins) = layer.content.get("enabledPlugins").or_else(|| {
            (workspace.agent_id.as_str() == "codex").then(|| layer.content.get("plugins"))?
        }) else {
            continue;
        };
        let Some(plugins) = plugins.as_object() else {
            continue;
        };
        for (logical_id, value) in plugins {
            observations.push(plugin_observation(
                workspace,
                layer,
                logical_id,
                plugin_enabled(value),
            ));
        }
    }
    if let Some(manifest) = runtime_manifest {
        for (logical_id, enabled) in &manifest.project_overlay.enabled_plugins {
            observations.push(runtime_plugin_observation(workspace, logical_id, *enabled));
        }
    }
    let diagnostics = inspect_plugin_health(workspace, &mut observations)?;
    Ok(collection_inventory(
        workspace,
        ResourceKind::Plugins,
        observations,
        version_diagnostic,
        diagnostics,
    ))
}

#[derive(Clone)]
pub(super) struct CollectionObservation {
    pub resource: ResourceRef,
    pub layer: ResourceLayer,
    pub source_id: String,
    pub target_id: PhysicalTargetId,
    pub logical_id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub ownership: ResourceOwnershipKind,
    pub ownership_record: Option<ResourceOwnershipRecord>,
    pub health: ResourceHealthView,
    pub configured: bool,
    pub artifact_id: Option<String>,
    pub resettable: bool,
    pub source: Option<ResourceSourceView>,
}

fn inspect_plugin_health(
    workspace: &WorkspaceDescriptor,
    observations: &mut [CollectionObservation],
) -> Result<Vec<ItemDiagnostic>, AgentError> {
    let registry = builtin_registry();
    let context = effective_context(workspace);
    let Some(port) = registry.adapter_for_context(&context)?.plugins() else {
        return Ok(Vec::new());
    };
    let mut diagnostics = Vec::new();
    match port.list(&context) {
        Ok(snapshots) => {
            for snapshot in snapshots {
                if snapshot
                    .content
                    .get("inspectionError")
                    .and_then(Value::as_str)
                    .is_none()
                {
                    continue;
                }
                let diagnostic = ItemDiagnostic {
                    code: "plugin_inspection_failed".into(),
                    message_key: "agents.inventory.pluginInspectionFailed".into(),
                    retryable: false,
                    resource_key: None,
                };
                for observation in observations
                    .iter_mut()
                    .filter(|observation| observation.logical_id == snapshot.resource.logical_id)
                {
                    observation.health = ResourceHealthView {
                        status: ResourceHealthStatus::Degraded,
                        diagnostic: Some(diagnostic.clone()),
                    };
                }
                diagnostics.push(diagnostic);
            }
        }
        Err(error) => diagnostics.push(diagnostic_from_error(
            "plugin_health_inspection_failed",
            &error,
        )),
    }
    Ok(diagnostics)
}

fn plugin_observation(
    workspace: &WorkspaceDescriptor,
    layer: &SettingsLayerSemantic,
    logical_id: &str,
    enabled: bool,
) -> CollectionObservation {
    let scope = if layer.layer == ResourceLayer::User {
        ResourceScope::User
    } else {
        ResourceScope::Project
    };
    let resource = ResourceRef {
        installation_id: if layer.layer == ResourceLayer::Runtime {
            workspace.effective_installation_id.clone()
        } else {
            workspace.base_installation_id.clone()
        },
        project_path: (scope == ResourceScope::Project)
            .then(|| workspace.canonical_project_path.clone()),
        kind: ResourceKind::Plugins,
        scope,
        logical_id: logical_id.to_owned(),
    };
    CollectionObservation {
        resource,
        layer: layer.layer,
        source_id: format!("agent-plugin:{logical_id}"),
        target_id: declaration_target_id(&workspace.key, &layer.declaration_key),
        logical_id: logical_id.to_owned(),
        display_name: logical_id.to_owned(),
        description: None,
        enabled,
        ownership: ResourceOwnershipKind::AgentManaged,
        ownership_record: None,
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
        configured: true,
        artifact_id: None,
        resettable: layer.logical_id == "project-local",
        source: None,
    }
}

fn runtime_plugin_observation(
    workspace: &WorkspaceDescriptor,
    logical_id: &str,
    enabled: bool,
) -> CollectionObservation {
    let resource = ResourceRef {
        installation_id: workspace.effective_installation_id.clone(),
        project_path: Some(workspace.canonical_project_path.clone()),
        kind: ResourceKind::Plugins,
        scope: ResourceScope::Project,
        logical_id: logical_id.to_owned(),
    };
    let declaration = DeclarationKey::from(opaque_contract_id(
        "runtime-plugin-declaration",
        &[workspace.key.as_str(), logical_id],
    ));
    CollectionObservation {
        resource,
        layer: ResourceLayer::Runtime,
        source_id: format!("agent-plugin:{logical_id}"),
        target_id: declaration_target_id(&workspace.key, &declaration),
        logical_id: logical_id.to_owned(),
        display_name: logical_id.to_owned(),
        description: None,
        enabled,
        ownership: ResourceOwnershipKind::Unknown,
        ownership_record: None,
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
        configured: true,
        artifact_id: None,
        resettable: true,
        source: None,
    }
}

pub(super) fn collection_inventory(
    workspace: &WorkspaceDescriptor,
    kind: ResourceKind,
    observations: Vec<CollectionObservation>,
    version_diagnostic: &ItemDiagnostic,
    mut diagnostics: Vec<ItemDiagnostic>,
) -> CollectionResourceInventory {
    let observed = observations.len();
    let mut groups = BTreeMap::<(String, String), Vec<CollectionObservation>>::new();
    for observation in observations {
        groups
            .entry((
                observation.logical_id.clone(),
                observation.source_id.clone(),
            ))
            .or_default()
            .push(observation);
    }
    let conflicts = groups
        .keys()
        .fold(BTreeMap::<String, usize>::new(), |mut counts, key| {
            *counts.entry(key.0.clone()).or_default() += 1;
            counts
        });
    let mut resources = groups
        .into_iter()
        .filter_map(|((logical_id, source_id), mut declarations)| {
            declarations
                .sort_by_key(|declaration| (declaration.configured, layer_rank(declaration.layer)));
            let winner = declarations.last()?.clone();
            let configured = declarations
                .iter()
                .filter(|declaration| declaration.configured)
                .cloned()
                .collect::<Vec<_>>();
            let configured_winner = configured.last();
            let available_artifact = declarations
                .iter()
                .find(|declaration| !declaration.configured)
                .and_then(|declaration| declaration.artifact_id.as_deref());
            let resource_key = ResourceKey::for_collection(
                &workspace.key,
                &workspace.agent_id,
                kind,
                &logical_id,
                &source_id,
            );
            let source = match source_for_group(&declarations) {
                Ok(source) => source,
                Err(()) => {
                    diagnostics.push(ItemDiagnostic {
                        code: "resource_source_conflict".into(),
                        message_key: "agents.inventory.resourceSourceConflict".into(),
                        retryable: false,
                        resource_key: Some(resource_key.clone()),
                    });
                    None
                }
            };
            let declaration_views = configured
                .iter()
                .map(|declaration| ResourceDeclarationView {
                    key: DeclarationKey::for_layer(
                        &resource_key,
                        declaration.layer,
                        declaration.source_id.as_str(),
                    ),
                    layer: declaration.layer,
                    source_id: declaration.source_id.clone(),
                    target_id: declaration.target_id.clone(),
                    scope: Some(declaration.resource.scope),
                })
                .collect::<Vec<_>>();
            let winner_key = declaration_views
                .last()
                .map(|declaration| declaration.key.clone());
            let conflict = conflicts.get(&logical_id).copied().unwrap_or_default() > 1;
            let ownership_record =
                configured_winner.and_then(|declaration| declaration.ownership_record.as_ref());
            let ownership = configured_winner
                .map(|declaration| declaration.ownership)
                .unwrap_or(winner.ownership);
            let effective_state = if conflict {
                EffectiveResourceState::Conflict
            } else if configured.is_empty() {
                EffectiveResourceState::Unconfigured
            } else if configured_winner.is_some_and(|winner| winner.enabled) {
                EffectiveResourceState::Enabled
            } else {
                EffectiveResourceState::Disabled
            };
            let has_resettable_declaration =
                configured.iter().any(|declaration| declaration.resettable);
            let management = resource_management(CollectionManagementInput {
                workspace,
                kind,
                state: effective_state,
                ownership,
                has_health_error: configured_winner
                    .is_some_and(|winner| winner.health.status == ResourceHealthStatus::Error),
                owned_artifact: ownership_record.map(|record| record.artifact_id.as_str()),
                owned_source_binding: ownership_record
                    .is_some_and(|record| record.source_binding.is_some()),
                available_artifact,
                has_resettable_declaration,
            });
            Some(CollectionResourceView {
                key: resource_key,
                kind,
                logical_id,
                display_name: winner.display_name,
                description: winner.description,
                effective_state,
                provenance: ResourceProvenanceView {
                    declarations: declaration_views,
                    winner: winner_key,
                    source,
                },
                ownership: ResourceOwnershipView {
                    kind: ownership,
                    record_id: ownership_record.map(|record| record.id.clone()),
                },
                health: configured_winner
                    .map(|winner| winner.health.clone())
                    .unwrap_or(winner.health),
                management,
            })
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| {
        left.logical_id
            .cmp(&right.logical_id)
            .then_with(|| left.key.cmp(&right.key))
    });
    diagnostics.insert(0, version_diagnostic.clone());
    CollectionResourceInventory {
        workspace_key: workspace.key.clone(),
        agent_id: workspace.agent_id.clone(),
        kind,
        coverage: CategoryCoverage {
            status: CoverageStatus::Partial,
            observed,
            visible: resources.len(),
            diagnostics,
        },
        resources,
    }
}

fn source_for_group(
    declarations: &[CollectionObservation],
) -> Result<Option<ResourceSourceView>, ()> {
    let catalog_source = unique_source(declarations.iter().filter_map(|declaration| {
        declaration.source.as_ref().filter(|source| {
            matches!(
                source.kind,
                ResourceSourceKind::CatalogGit | ResourceSourceKind::CatalogLocal
            )
        })
    }))?;
    if let Some(source) = catalog_source {
        return Ok(Some(source.clone()));
    }

    unique_source(
        declarations
            .iter()
            .filter_map(|declaration| declaration.source.as_ref())
            .filter(|source| source.kind == ResourceSourceKind::InstalledPath),
    )
    .map(|source| source.cloned())
}

fn unique_source<'a>(
    mut sources: impl Iterator<Item = &'a ResourceSourceView>,
) -> Result<Option<&'a ResourceSourceView>, ()> {
    let selected = sources.next();
    if selected.is_some_and(|selected| sources.any(|source| source != selected)) {
        return Err(());
    }
    Ok(selected)
}

pub(super) fn failed_collection(
    workspace: &WorkspaceDescriptor,
    kind: ResourceKind,
    diagnostic: ItemDiagnostic,
) -> CollectionResourceInventory {
    CollectionResourceInventory {
        workspace_key: workspace.key.clone(),
        agent_id: workspace.agent_id.clone(),
        kind,
        coverage: CategoryCoverage {
            status: CoverageStatus::Failed,
            observed: 0,
            visible: 0,
            diagnostics: vec![diagnostic],
        },
        resources: Vec::new(),
    }
}

fn effective_context(workspace: &WorkspaceDescriptor) -> AgentContext {
    AgentContext {
        installation_id: workspace.effective_installation_id.clone(),
        project_path: Some(workspace.canonical_project_path.clone()),
    }
}

fn plugin_enabled(value: &Value) -> bool {
    value
        .as_bool()
        .or_else(|| value.get("enabled").and_then(Value::as_bool))
        .unwrap_or(true)
}

pub(super) fn layer_for_scope(scope: ResourceScope) -> ResourceLayer {
    match scope {
        ResourceScope::User => ResourceLayer::User,
        ResourceScope::Project => ResourceLayer::Project,
    }
}

fn layer_rank(layer: ResourceLayer) -> u8 {
    match layer {
        ResourceLayer::System => 0,
        ResourceLayer::User => 1,
        ResourceLayer::Project => 2,
        ResourceLayer::Runtime => 3,
    }
}

pub(super) fn opaque_source_id(path: &str) -> String {
    opaque_contract_id("resource-source", &[path])
}

fn declaration_target_id(
    workspace_key: &WorkspaceKey,
    declaration_key: &DeclarationKey,
) -> PhysicalTargetId {
    PhysicalTargetId::from(opaque_contract_id(
        "target-declaration",
        &[workspace_key.as_str(), declaration_key.as_str()],
    ))
}

fn diagnostic_from_error(code: &str, error: &AgentError) -> ItemDiagnostic {
    ItemDiagnostic {
        code: code.into(),
        message_key: format!("agents.inventory.{code}"),
        retryable: error.retryable,
        resource_key: None,
    }
}

pub(super) fn inventory_error(
    workspace: &WorkspaceDescriptor,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.effective_installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "collection_inventory"})),
    }
}

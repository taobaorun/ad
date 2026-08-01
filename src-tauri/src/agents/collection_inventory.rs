use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use super::settings_inventory::SettingsLayerSemantic;
use super::{
    builtin_registry, opaque_contract_id, AgentContext, AgentError, AgentErrorCode,
    CategoryCoverage, CollectionResourceInventory, CollectionResourceView, CoverageStatus,
    DeclarationKey, EffectiveResourceState, ItemDiagnostic, PhysicalTargetId,
    ProjectCodexRuntimeManifest, ResourceAction, ResourceActionAvailability, ResourceActionView,
    ResourceDeclarationView, ResourceHealthStatus, ResourceHealthView, ResourceKey, ResourceKind,
    ResourceLayer, ResourceManagementStatus, ResourceManagementView, ResourceOwnershipKind,
    ResourceOwnershipView, ResourceProvenanceView, ResourceRef, ResourceScope, ResourceSnapshot,
    WorkspaceDescriptor, WorkspaceKey,
};

pub(super) fn inspect_skills(
    workspace: &WorkspaceDescriptor,
    version_diagnostic: &ItemDiagnostic,
) -> Result<CollectionResourceInventory, AgentError> {
    let registry = builtin_registry();
    let context = effective_context(workspace);
    let adapter = registry.adapter_for_context(&context)?;
    let port = adapter
        .skills()
        .ok_or_else(|| inventory_error(workspace, "Agent does not support Skill inspection"))?;
    let snapshots = port.list(&context)?;
    let mut observations = snapshots
        .into_iter()
        .filter_map(skill_observation)
        .collect::<Vec<_>>();
    if workspace.agent_id.as_str() == "claude-code" {
        let known_project_names = observations
            .iter()
            .filter(|observation| observation.layer == ResourceLayer::Project)
            .map(|observation| observation.logical_id.clone())
            .collect::<BTreeSet<_>>();
        observations.extend(
            scan_claude_project_skills(workspace)?
                .into_iter()
                .filter(|observation| !known_project_names.contains(&observation.logical_id)),
        );
    }
    Ok(collection_inventory(
        workspace,
        ResourceKind::Skills,
        observations,
        version_diagnostic,
        Vec::new(),
    ))
}

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
struct CollectionObservation {
    resource: ResourceRef,
    layer: ResourceLayer,
    source_id: String,
    target_id: PhysicalTargetId,
    logical_id: String,
    display_name: String,
    description: Option<String>,
    enabled: bool,
    ownership: ResourceOwnershipKind,
    health: ResourceHealthView,
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

fn skill_observation(snapshot: ResourceSnapshot) -> Option<CollectionObservation> {
    if snapshot.content.get("scope").and_then(Value::as_str) == Some("none") {
        return None;
    }
    let logical_id = snapshot
        .content
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(snapshot.resource.logical_id.as_str())
        .to_owned();
    let source_id = snapshot
        .content
        .get("sourceId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| opaque_source_id(&snapshot.location.path));
    let description = snapshot
        .content
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let enabled = snapshot
        .content
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let ownership = match snapshot.content.get("source").and_then(Value::as_str) {
        Some("external") => ResourceOwnershipKind::External,
        _ => ResourceOwnershipKind::Unknown,
    };
    Some(CollectionObservation {
        target_id: PhysicalTargetId::for_resource(&snapshot.resource),
        layer: layer_for_scope(snapshot.resource.scope),
        resource: snapshot.resource,
        source_id,
        logical_id: logical_id.clone(),
        display_name: logical_id,
        description,
        enabled,
        ownership,
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
    })
}

fn scan_claude_project_skills(
    workspace: &WorkspaceDescriptor,
) -> Result<Vec<CollectionObservation>, AgentError> {
    let root = Path::new(&workspace.canonical_project_path).join(".claude/skills");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(inventory_error(
                workspace,
                format!("Failed to scan Project Skills: {error}"),
            ))
        }
    };
    let mut observations = Vec::new();
    for entry in entries.flatten() {
        let logical_id = entry.file_name().to_string_lossy().into_owned();
        if logical_id.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let resource = ResourceRef {
            installation_id: workspace.base_installation_id.clone(),
            project_path: Some(workspace.canonical_project_path.clone()),
            kind: ResourceKind::Skills,
            scope: ResourceScope::Project,
            logical_id: logical_id.clone(),
        };
        let canonical_source = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let healthy = canonical_source.join("SKILL.md").is_file();
        let diagnostic = (!healthy).then(|| ItemDiagnostic {
            code: "invalid_skill_directory".into(),
            message_key: "agents.inventory.invalidSkillDirectory".into(),
            retryable: false,
            resource_key: None,
        });
        observations.push(CollectionObservation {
            target_id: PhysicalTargetId::for_resource(&resource),
            resource,
            layer: ResourceLayer::Project,
            source_id: opaque_source_id(&canonical_source.to_string_lossy()),
            logical_id: logical_id.clone(),
            display_name: logical_id,
            description: None,
            enabled: healthy,
            ownership: ResourceOwnershipKind::External,
            health: ResourceHealthView {
                status: if healthy {
                    ResourceHealthStatus::Healthy
                } else {
                    ResourceHealthStatus::Degraded
                },
                diagnostic,
            },
        });
    }
    Ok(observations)
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
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
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
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
    }
}

fn collection_inventory(
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
            declarations.sort_by_key(|declaration| layer_rank(declaration.layer));
            let winner = declarations.last()?.clone();
            let resource_key = ResourceKey::for_collection(
                &workspace.key,
                &workspace.agent_id,
                kind,
                &logical_id,
                &source_id,
            );
            let declaration_views = declarations
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
            Some(CollectionResourceView {
                key: resource_key,
                kind,
                logical_id,
                display_name: winner.display_name,
                description: winner.description,
                effective_state: if conflict {
                    EffectiveResourceState::Conflict
                } else if winner.enabled {
                    EffectiveResourceState::Enabled
                } else {
                    EffectiveResourceState::Disabled
                },
                provenance: ResourceProvenanceView {
                    declarations: declaration_views,
                    winner: winner_key,
                },
                ownership: ResourceOwnershipView {
                    kind: winner.ownership,
                    record_id: None,
                },
                health: winner.health,
                management: ResourceManagementView {
                    status: ResourceManagementStatus::ReadOnly,
                    actions: vec![ResourceActionView {
                        action: ResourceAction::Inspect,
                        availability: ResourceActionAvailability::Available,
                        limitation: None,
                    }],
                },
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

fn layer_for_scope(scope: ResourceScope) -> ResourceLayer {
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

fn opaque_source_id(path: &str) -> String {
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

fn inventory_error(workspace: &WorkspaceDescriptor, message: impl Into<String>) -> AgentError {
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

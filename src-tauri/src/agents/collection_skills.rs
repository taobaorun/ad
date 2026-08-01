use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::collection_inventory::{
    collection_inventory, inventory_error, layer_for_scope, opaque_source_id, CollectionObservation,
};
use super::execution_state::ExecutionState;
use super::{
    builtin_registry, load_ownership_record, load_skill_catalog_snapshot,
    validate_ownership_artifact, validate_ownership_record, verify_skill_artifact, AgentContext,
    AgentError, CollectionResourceInventory, ContentDigest, ItemDiagnostic, PhysicalTargetId,
    ResourceHealthStatus, ResourceHealthView, ResourceKind, ResourceLayer, ResourceOwnershipKind,
    ResourceOwnershipRecord, ResourceRef, ResourceScope, ResourceSnapshot, ResourceStateKind,
    WorkspaceDescriptor,
};

pub(super) fn inspect_skills(
    workspace: &WorkspaceDescriptor,
    version_diagnostic: &ItemDiagnostic,
) -> Result<CollectionResourceInventory, AgentError> {
    let registry = builtin_registry();
    let context = AgentContext {
        installation_id: workspace.effective_installation_id.clone(),
        project_path: Some(workspace.canonical_project_path.clone()),
    };
    let adapter = registry.adapter_for_context(&context)?;
    let port = adapter
        .skills()
        .ok_or_else(|| inventory_error(workspace, "Agent does not support Skill inspection"))?;
    let ownership = project_ownership_records(workspace)?;
    let mut observations = port
        .list(&context)?
        .into_iter()
        .filter_map(|snapshot| skill_observation(snapshot, &ownership))
        .collect::<Vec<_>>();
    if workspace.agent_id.as_str() == "claude-code" {
        let known_project_names = observations
            .iter()
            .filter(|observation| observation.layer == ResourceLayer::Project)
            .map(|observation| observation.logical_id.clone())
            .collect::<BTreeSet<_>>();
        observations.extend(
            scan_claude_project_skills(workspace, &ownership)?
                .into_iter()
                .filter(|observation| !known_project_names.contains(&observation.logical_id)),
        );
    }
    let (catalog, diagnostics) = catalog_skill_observations(workspace);
    observations.extend(catalog);
    Ok(collection_inventory(
        workspace,
        ResourceKind::Skills,
        observations,
        version_diagnostic,
        diagnostics,
    ))
}

fn skill_observation(
    snapshot: ResourceSnapshot,
    ownership: &[ResourceOwnershipRecord],
) -> Option<CollectionObservation> {
    if snapshot.content.get("scope").and_then(Value::as_str) == Some("none") {
        return None;
    }
    let logical_id = snapshot
        .content
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(snapshot.resource.logical_id.as_str())
        .to_owned();
    let mut source_id = snapshot
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
    let mut ownership_kind = match snapshot.content.get("source").and_then(Value::as_str) {
        Some("external") => ResourceOwnershipKind::External,
        _ => ResourceOwnershipKind::Unknown,
    };
    let mut ownership_record = ownership
        .iter()
        .find(|record| record.target_path == snapshot.location.path)
        .cloned();
    let mut health = ResourceHealthView {
        status: ResourceHealthStatus::Healthy,
        diagnostic: None,
    };
    if let Some(record) = ownership_record.as_ref() {
        match validate_record_for_inventory(record) {
            Ok(()) => {
                ownership_kind = ResourceOwnershipKind::AdManaged;
                source_id = record
                    .resource
                    .logical_id
                    .rsplit_once('/')
                    .map(|(source, _)| source.to_owned())
                    .unwrap_or_else(|| source_id.clone());
            }
            Err(diagnostic) => {
                ownership_kind = ResourceOwnershipKind::Unknown;
                ownership_record = None;
                health = ResourceHealthView {
                    status: ResourceHealthStatus::Error,
                    diagnostic: Some(diagnostic),
                };
            }
        }
    }
    Some(CollectionObservation {
        target_id: PhysicalTargetId::for_resource(&snapshot.resource),
        layer: layer_for_scope(snapshot.resource.scope),
        resource: snapshot.resource,
        source_id,
        logical_id: logical_id.clone(),
        display_name: logical_id,
        description,
        enabled,
        ownership: ownership_kind,
        artifact_id: ownership_record
            .as_ref()
            .map(|record| record.artifact_id.clone()),
        ownership_record,
        health,
        configured: true,
        resettable: false,
    })
}

fn scan_claude_project_skills(
    workspace: &WorkspaceDescriptor,
    ownership: &[ResourceOwnershipRecord],
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
        let mut ownership_record = ownership
            .iter()
            .find(|record| record.target_path == path.to_string_lossy())
            .cloned();
        let ownership_kind = if ownership_record
            .as_ref()
            .is_some_and(|record| validate_record_for_inventory(record).is_ok())
        {
            ResourceOwnershipKind::AdManaged
        } else {
            ownership_record = None;
            ResourceOwnershipKind::External
        };
        observations.push(CollectionObservation {
            target_id: PhysicalTargetId::for_resource(&resource),
            resource,
            layer: ResourceLayer::Project,
            source_id: ownership_record
                .as_ref()
                .and_then(|record| record.resource.logical_id.rsplit_once('/'))
                .map(|(source, _)| source.to_owned())
                .unwrap_or_else(|| opaque_source_id(&canonical_source.to_string_lossy())),
            logical_id: logical_id.clone(),
            display_name: logical_id,
            description: None,
            enabled: healthy,
            ownership: ownership_kind,
            artifact_id: ownership_record
                .as_ref()
                .map(|record| record.artifact_id.clone()),
            ownership_record,
            health: ResourceHealthView {
                status: if healthy {
                    ResourceHealthStatus::Healthy
                } else {
                    ResourceHealthStatus::Degraded
                },
                diagnostic,
            },
            configured: true,
            resettable: false,
        });
    }
    Ok(observations)
}

pub(super) fn project_ownership_records(
    workspace: &WorkspaceDescriptor,
) -> Result<Vec<ResourceOwnershipRecord>, AgentError> {
    let state =
        ExecutionState::open().map_err(|error| inventory_error(workspace, error.to_string()))?;
    let mut records = Vec::new();
    for name in state
        .ownership()
        .entry_names()
        .map_err(|error| inventory_error(workspace, error.to_string()))?
    {
        let Some(name) = name.to_str() else { continue };
        let bytes = state
            .ownership()
            .read(name)
            .map_err(|error| inventory_error(workspace, error.to_string()))?;
        let Ok(candidate) = serde_json::from_slice::<ResourceOwnershipRecord>(&bytes) else {
            continue;
        };
        if candidate.workspace_key != workspace.key
            || candidate.resource.kind != ResourceKind::Skills
        {
            continue;
        }
        if let Some(record) = load_ownership_record(&state, &candidate.resource)? {
            records.push(record);
        }
    }
    Ok(records)
}

fn validate_record_for_inventory(record: &ResourceOwnershipRecord) -> Result<(), ItemDiagnostic> {
    let target = Path::new(&record.target_path);
    let metadata = std::fs::symlink_metadata(target).map_err(|_| ownership_diagnostic())?;
    let digest = if metadata.file_type().is_symlink() {
        let link = std::fs::read_link(target).map_err(|_| ownership_diagnostic())?;
        Some(ContentDigest::sha256(link.to_string_lossy().as_bytes()))
    } else {
        return Err(ownership_diagnostic());
    };
    validate_ownership_record(
        record,
        &record.resource,
        target,
        ResourceStateKind::Symlink,
        digest.as_ref(),
    )
    .and_then(|_| validate_ownership_artifact(record))
    .map_err(|_| ownership_diagnostic())
}

fn ownership_diagnostic() -> ItemDiagnostic {
    ItemDiagnostic {
        code: "skill_ownership_invalid".into(),
        message_key: "agents.inventory.skillOwnershipInvalid".into(),
        retryable: false,
        resource_key: None,
    }
}

fn catalog_skill_observations(
    workspace: &WorkspaceDescriptor,
) -> (Vec<CollectionObservation>, Vec<ItemDiagnostic>) {
    let catalog = match load_skill_catalog_snapshot() {
        Ok(catalog) => catalog,
        Err(_) => {
            return (
                Vec::new(),
                vec![ItemDiagnostic {
                    code: "skill_catalog_unavailable".into(),
                    message_key: "agents.inventory.skillCatalogUnavailable".into(),
                    retryable: false,
                    resource_key: None,
                }],
            )
        }
    };
    let mut observations = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in catalog.entries {
        let tree = match verify_skill_artifact(&entry.current_artifact) {
            Ok(tree) => tree,
            Err(_) => {
                diagnostics.push(ItemDiagnostic {
                    code: "skill_artifact_invalid".into(),
                    message_key: "agents.inventory.skillArtifactInvalid".into(),
                    retryable: false,
                    resource_key: None,
                });
                continue;
            }
        };
        for skill in &entry.current_artifact.skills {
            let resource = ResourceRef {
                installation_id: workspace.effective_installation_id.clone(),
                project_path: Some(workspace.canonical_project_path.clone()),
                kind: ResourceKind::Skills,
                scope: ResourceScope::Project,
                logical_id: format!("{}/{}", entry.source_id, skill.logical_id),
            };
            observations.push(CollectionObservation {
                target_id: PhysicalTargetId::for_resource(&resource),
                resource,
                layer: ResourceLayer::Project,
                source_id: entry.source_id.clone(),
                logical_id: skill.logical_id.clone(),
                display_name: skill.logical_id.clone(),
                description: Some(entry.display_name.clone()),
                enabled: false,
                ownership: ResourceOwnershipKind::AdManaged,
                ownership_record: None,
                health: ResourceHealthView {
                    status: ResourceHealthStatus::Healthy,
                    diagnostic: None,
                },
                configured: false,
                artifact_id: Some(tree.join(&skill.subpath).to_string_lossy().into_owned()),
                resettable: false,
            });
        }
    }
    (observations, diagnostics)
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::execution_state::ExecutionState;
use super::user_plugins::{inspect_user_plugin_native_id, list_user_plugin_management_records};
use super::{
    builtin_registry, load_ownership_record, load_resource_catalog_snapshot, opaque_contract_id,
    resolve_user_agent_workspace, validate_ownership_artifact, validate_ownership_record,
    AgentContext, AgentError, AgentErrorCode, CategoryCoverage, CollectionResourceInventory,
    CollectionResourceView, ContentDigest, CoverageStatus, DeclarationKey, EffectiveResourceState,
    InstallationId, InventoryRevision, ItemDiagnostic, PhysicalTargetId, ResourceAction,
    ResourceActionAvailability, ResourceActionIntent, ResourceActionView, ResourceDeclarationView,
    ResourceHealthStatus, ResourceHealthView, ResourceKey, ResourceKind, ResourceLayer,
    ResourceLifecycle, ResourceManagementStatus, ResourceManagementView, ResourceOwnershipKind,
    ResourceOwnershipRecord, ResourceOwnershipView, ResourceRef, ResourceScope, ResourceSnapshot,
    ResourceSourceKind, ResourceSourceView, ResourceStateKind, SkillSourceType,
    UserResourceInventory, UserWorkspaceDescriptor,
};

pub fn inspect_user_resource_inventory(
    installation_id: &InstallationId,
) -> Result<UserResourceInventory, AgentError> {
    let workspace = resolve_user_agent_workspace(installation_id)?;
    let context = AgentContext {
        installation_id: installation_id.clone(),
        project_path: None,
    };
    let registry = builtin_registry();
    let adapter = registry.adapter_for_context(&context)?;
    let skills_port = adapter
        .skills()
        .ok_or_else(|| inventory_error(&workspace, "Agent does not support Skill inspection"))?;
    let plugins_port = adapter
        .plugins()
        .ok_or_else(|| inventory_error(&workspace, "Agent does not support Plugin inspection"))?;
    let skills = inspect_user_skills(&workspace, &context, skills_port.list(&context)?)?;
    let plugins = inspect_user_plugins(&workspace, plugins_port.list(&context)?)?;
    let revision = inventory_revision(&workspace, &skills, &plugins)?;
    Ok(UserResourceInventory {
        schema_version: 1,
        workspace,
        revision,
        skills,
        plugins,
        diagnostics: Vec::new(),
    })
}

fn inspect_user_skills(
    workspace: &UserWorkspaceDescriptor,
    context: &AgentContext,
    snapshots: Vec<ResourceSnapshot>,
) -> Result<CollectionResourceInventory, AgentError> {
    let snapshots = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.resource.scope == ResourceScope::User)
        .collect::<Vec<_>>();
    let ownership = user_ownership_records_for(workspace, ResourceKind::Skills)?;
    let catalog = load_resource_catalog_snapshot()
        .map_err(|error| inventory_error(workspace, error.to_string()))?;
    let registry = builtin_registry();
    let adapter = registry.adapter_for_context(context)?;
    let port = adapter
        .skills()
        .ok_or_else(|| inventory_error(workspace, "Agent does not support Skill inspection"))?;
    let mut matched_paths = HashSet::new();
    let mut resources = Vec::new();
    let mut diagnostics = Vec::new();

    for candidate in catalog.resources.values().filter(|resource| {
        resource.kind == ResourceKind::Skills
            && resource.present
            && resource.lifecycle == ResourceLifecycle::Managed
    }) {
        let resource = ResourceRef {
            installation_id: workspace.installation_id.clone(),
            project_path: None,
            kind: ResourceKind::Skills,
            scope: ResourceScope::User,
            logical_id: format!("{}/{}", candidate.source_id, candidate.install_id),
        };
        let target = port.resolve(context, &resource)?.path().to_path_buf();
        let target_path = target.to_string_lossy().into_owned();
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.location.path == target_path);
        if snapshot.is_some() {
            matched_paths.insert(target_path.clone());
        }
        let record = ownership
            .iter()
            .find(|record| {
                record.target_path == target_path
                    && record.catalog_binding.as_ref().is_some_and(|binding| {
                        binding.resource_id == candidate.id
                            && binding.source_id == candidate.source_id
                            && binding.install_id == candidate.install_id
                            && binding.resource_kind == ResourceKind::Skills
                    })
            })
            .filter(|record| validate_skill_record(record).is_ok());
        let target_exists = std::fs::symlink_metadata(&target).is_ok();
        let configured = snapshot.is_some() || target_exists;
        let ownership_kind = if configured {
            if record.is_some() {
                ResourceOwnershipKind::AdManaged
            } else {
                ResourceOwnershipKind::External
            }
        } else {
            ResourceOwnershipKind::AdManaged
        };
        let enabled = snapshot
            .and_then(|snapshot| snapshot.content.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(configured);
        let available_digest = current_user_skill_digest(&candidate.id).ok();
        let health = if configured && snapshot.is_none() {
            let diagnostic = ItemDiagnostic {
                code: "invalid_skill_directory".into(),
                message_key: "agents.inventory.invalidSkillDirectory".into(),
                retryable: false,
                resource_key: None,
            };
            diagnostics.push(diagnostic.clone());
            ResourceHealthView {
                status: ResourceHealthStatus::Degraded,
                diagnostic: Some(diagnostic),
            }
        } else {
            ResourceHealthView {
                status: ResourceHealthStatus::Healthy,
                diagnostic: None,
            }
        };
        let source = catalog
            .sources
            .get(&candidate.source_id)
            .and_then(catalog_source_view);
        let key = ResourceKey::for_collection(
            &workspace.key,
            &workspace.agent_id,
            ResourceKind::Skills,
            &candidate.install_id,
            &candidate.source_id,
        );
        let effective_state = if !configured {
            EffectiveResourceState::Unconfigured
        } else if enabled {
            EffectiveResourceState::Enabled
        } else {
            EffectiveResourceState::Disabled
        };
        resources.push(CollectionResourceView {
            key: key.clone(),
            kind: ResourceKind::Skills,
            logical_id: candidate.install_id.clone(),
            display_name: candidate.display_name.clone(),
            description: candidate.description.clone(),
            effective_state,
            provenance: super::ResourceProvenanceView {
                declarations: configured
                    .then(|| ResourceDeclarationView {
                        key: DeclarationKey::for_layer(
                            &key,
                            ResourceLayer::User,
                            &candidate.source_id,
                        ),
                        layer: ResourceLayer::User,
                        source_id: candidate.source_id.clone(),
                        target_id: PhysicalTargetId::for_resource(&resource),
                        scope: Some(ResourceScope::User),
                    })
                    .into_iter()
                    .collect(),
                winner: configured.then(|| {
                    DeclarationKey::for_layer(&key, ResourceLayer::User, &candidate.source_id)
                }),
                source,
            },
            ownership: ResourceOwnershipView {
                kind: ownership_kind,
                record_id: record.map(|record| record.id.clone()),
            },
            health,
            management: user_skill_management(
                ownership_kind,
                effective_state,
                record,
                available_digest.as_ref(),
            ),
        });
    }

    for snapshot in snapshots
        .iter()
        .filter(|snapshot| !matched_paths.contains(&snapshot.location.path))
    {
        resources.push(external_snapshot_view(workspace, snapshot));
    }
    resources.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    Ok(CollectionResourceInventory {
        workspace_key: workspace.key.clone(),
        agent_id: workspace.agent_id.clone(),
        kind: ResourceKind::Skills,
        coverage: CategoryCoverage {
            status: CoverageStatus::Partial,
            observed: snapshots.len(),
            visible: resources.len(),
            diagnostics,
        },
        resources,
    })
}

fn inspect_user_plugins(
    workspace: &UserWorkspaceDescriptor,
    snapshots: Vec<ResourceSnapshot>,
) -> Result<CollectionResourceInventory, AgentError> {
    let snapshots = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.resource.scope == ResourceScope::User)
        .collect::<Vec<_>>();
    let catalog = load_resource_catalog_snapshot()
        .map_err(|error| inventory_error(workspace, error.to_string()))?;
    let records = list_user_plugin_management_records(workspace)?;
    let mut matched_native_ids = HashSet::new();
    let mut resources = Vec::new();
    let mut diagnostics = Vec::new();
    for candidate in catalog.resources.values().filter(|resource| {
        resource.kind == ResourceKind::Plugins
            && resource.present
            && resource.lifecycle == ResourceLifecycle::Managed
    }) {
        let proposed_native_id = inspect_user_plugin_native_id(workspace, &candidate.id);
        let record = records
            .iter()
            .find(|record| record.resource_id == candidate.id);
        let native_id = record
            .map(|record| record.native_id.clone())
            .or_else(|| proposed_native_id.as_ref().ok().cloned());
        let same_name = snapshots
            .iter()
            .filter(|snapshot| {
                plugin_install_name(&snapshot.resource.logical_id) == candidate.install_id
            })
            .collect::<Vec<_>>();
        let snapshot = native_id
            .as_ref()
            .and_then(|native_id| {
                same_name
                    .iter()
                    .find(|snapshot| snapshot.resource.logical_id == *native_id)
                    .copied()
            })
            .or_else(|| same_name.first().copied());
        matched_native_ids.extend(
            same_name
                .iter()
                .map(|snapshot| snapshot.resource.logical_id.clone()),
        );
        let configured = snapshot.is_some();
        let conflicting_external = native_id.as_ref().is_some_and(|native_id| {
            same_name
                .iter()
                .any(|snapshot| snapshot.resource.logical_id != *native_id)
        });
        let ownership_kind = if configured && (record.is_none() || conflicting_external) {
            ResourceOwnershipKind::External
        } else {
            ResourceOwnershipKind::AdManaged
        };
        let managed_record = (ownership_kind == ResourceOwnershipKind::AdManaged)
            .then_some(record)
            .flatten();
        let enabled = snapshot
            .and_then(|snapshot| snapshot.content.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(configured);
        let agent_supported = proposed_native_id.is_ok();
        let health = if managed_record.is_some() && !configured {
            let diagnostic = ItemDiagnostic {
                code: "managed_user_plugin_unavailable".into(),
                message_key: "agents.inventory.managedUserPluginUnavailable".into(),
                retryable: true,
                resource_key: None,
            };
            diagnostics.push(diagnostic.clone());
            ResourceHealthView {
                status: ResourceHealthStatus::Degraded,
                diagnostic: Some(diagnostic),
            }
        } else {
            ResourceHealthView {
                status: ResourceHealthStatus::Healthy,
                diagnostic: None,
            }
        };
        let source = catalog
            .sources
            .get(&candidate.source_id)
            .and_then(catalog_source_view);
        let key = ResourceKey::for_collection(
            &workspace.key,
            &workspace.agent_id,
            ResourceKind::Plugins,
            &candidate.install_id,
            &candidate.source_id,
        );
        let declaration_source = candidate.source_id.clone();
        let effective_state = if !configured {
            EffectiveResourceState::Unconfigured
        } else if enabled {
            EffectiveResourceState::Enabled
        } else {
            EffectiveResourceState::Disabled
        };
        let resource = ResourceRef {
            installation_id: workspace.installation_id.clone(),
            project_path: None,
            kind: ResourceKind::Plugins,
            scope: ResourceScope::User,
            logical_id: native_id
                .clone()
                .unwrap_or_else(|| format!("{}/{}", candidate.source_id, candidate.install_id)),
        };
        resources.push(CollectionResourceView {
            key: key.clone(),
            kind: ResourceKind::Plugins,
            logical_id: candidate.install_id.clone(),
            display_name: candidate.display_name.clone(),
            description: candidate.description.clone(),
            effective_state,
            provenance: super::ResourceProvenanceView {
                declarations: configured
                    .then(|| ResourceDeclarationView {
                        key: DeclarationKey::for_layer(
                            &key,
                            ResourceLayer::User,
                            &declaration_source,
                        ),
                        layer: ResourceLayer::User,
                        source_id: declaration_source.clone(),
                        target_id: PhysicalTargetId::for_resource(&resource),
                        scope: Some(ResourceScope::User),
                    })
                    .into_iter()
                    .collect(),
                winner: configured.then(|| {
                    DeclarationKey::for_layer(&key, ResourceLayer::User, &declaration_source)
                }),
                source,
            },
            ownership: ResourceOwnershipView {
                kind: ownership_kind,
                record_id: managed_record.map(|record| record.id.clone()),
            },
            health,
            management: user_plugin_management(
                ownership_kind,
                effective_state,
                agent_supported,
                managed_record.is_some(),
            ),
        });
    }
    resources.extend(
        snapshots
            .iter()
            .filter(|snapshot| !matched_native_ids.contains(&snapshot.resource.logical_id))
            .map(|snapshot| external_snapshot_view(workspace, snapshot)),
    );
    resources.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    Ok(CollectionResourceInventory {
        workspace_key: workspace.key.clone(),
        agent_id: workspace.agent_id.clone(),
        kind: ResourceKind::Plugins,
        coverage: CategoryCoverage {
            status: CoverageStatus::Partial,
            observed: snapshots.len(),
            visible: resources.len(),
            diagnostics,
        },
        resources,
    })
}

fn user_plugin_management(
    ownership: ResourceOwnershipKind,
    state: EffectiveResourceState,
    agent_supported: bool,
    has_record: bool,
) -> ResourceManagementView {
    if ownership == ResourceOwnershipKind::External {
        return external_management();
    }
    if state == EffectiveResourceState::Unconfigured {
        let mut actions = vec![
            inspect_action(),
            if agent_supported {
                confirmation_action(ResourceAction::Install)
            } else {
                unavailable_action(
                    ResourceAction::Install,
                    "unsupported_agent_capability",
                    "agents.resources.unsupportedAgentCapability",
                )
            },
        ];
        if has_record {
            actions.push(confirmation_action(ResourceAction::Remove));
        }
        return ResourceManagementView {
            status: ResourceManagementStatus::Managed,
            actions,
        };
    }
    if !has_record {
        return external_management();
    }
    ResourceManagementView {
        status: ResourceManagementStatus::Managed,
        actions: vec![
            inspect_action(),
            confirmation_action(if state == EffectiveResourceState::Enabled {
                ResourceAction::Disable
            } else {
                ResourceAction::Enable
            }),
            unavailable_action(
                ResourceAction::Update,
                "unsupported_agent_capability",
                "agents.resources.unsupportedAgentCapability",
            ),
            confirmation_action(ResourceAction::Remove),
        ],
    }
}

fn catalog_source_view(source: &super::CatalogSource) -> Option<ResourceSourceView> {
    super::skill_catalog::format_safe_source_location(source.source_type, &source.location).map(
        |location| ResourceSourceView {
            kind: match source.source_type {
                SkillSourceType::Git => ResourceSourceKind::CatalogGit,
                SkillSourceType::Local => ResourceSourceKind::CatalogLocal,
            },
            display_name: source.display_name.clone(),
            location,
            branch: source.branch.clone(),
            subdirectory: source.subdirectory.clone(),
        },
    )
}

fn plugin_install_name(native_id: &str) -> &str {
    native_id
        .split_once('@')
        .map_or(native_id, |(name, _)| name)
}

fn external_snapshot_view(
    workspace: &UserWorkspaceDescriptor,
    snapshot: &ResourceSnapshot,
) -> CollectionResourceView {
    let logical_id = snapshot
        .content
        .get("name")
        .or_else(|| snapshot.content.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(&snapshot.resource.logical_id)
        .to_owned();
    let source_id = format!(
        "agent-{}:{logical_id}",
        snapshot.resource.kind.contract_name()
    );
    let key = ResourceKey::for_collection(
        &workspace.key,
        &workspace.agent_id,
        snapshot.resource.kind,
        &logical_id,
        &source_id,
    );
    let enabled = snapshot
        .content
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let declaration = DeclarationKey::for_layer(&key, ResourceLayer::User, &source_id);
    CollectionResourceView {
        key,
        kind: snapshot.resource.kind,
        logical_id: logical_id.clone(),
        display_name: logical_id,
        description: snapshot
            .content
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        effective_state: if enabled {
            EffectiveResourceState::Enabled
        } else {
            EffectiveResourceState::Disabled
        },
        provenance: super::ResourceProvenanceView {
            declarations: vec![ResourceDeclarationView {
                key: declaration.clone(),
                layer: ResourceLayer::User,
                source_id,
                target_id: PhysicalTargetId::for_resource(&snapshot.resource),
                scope: Some(ResourceScope::User),
            }],
            winner: Some(declaration),
            source: None,
        },
        ownership: ResourceOwnershipView {
            kind: ResourceOwnershipKind::External,
            record_id: None,
        },
        health: ResourceHealthView {
            status: ResourceHealthStatus::Healthy,
            diagnostic: None,
        },
        management: external_management(),
    }
}

fn user_skill_management(
    ownership: ResourceOwnershipKind,
    state: EffectiveResourceState,
    record: Option<&ResourceOwnershipRecord>,
    available_digest: Option<&ContentDigest>,
) -> ResourceManagementView {
    if ownership == ResourceOwnershipKind::External {
        return external_management();
    }
    if state == EffectiveResourceState::Unconfigured {
        return ResourceManagementView {
            status: ResourceManagementStatus::Managed,
            actions: vec![
                inspect_action(),
                confirmation_action(ResourceAction::Install),
            ],
        };
    }
    let update = if record.is_some_and(|record| {
        available_digest.is_some_and(|digest| record.artifact_digest != *digest)
    }) {
        confirmation_action(ResourceAction::Update)
    } else {
        unavailable_action(
            ResourceAction::Update,
            "skill_revision_current",
            "agents.resources.skillRevisionCurrent",
        )
    };
    ResourceManagementView {
        status: ResourceManagementStatus::Managed,
        actions: vec![
            inspect_action(),
            confirmation_action(if state == EffectiveResourceState::Enabled {
                ResourceAction::Disable
            } else {
                ResourceAction::Enable
            }),
            update,
            confirmation_action(ResourceAction::Remove),
        ],
    }
}

fn external_management() -> ResourceManagementView {
    ResourceManagementView {
        status: ResourceManagementStatus::External,
        actions: vec![
            inspect_action(),
            ResourceActionView {
                action: ResourceAction::OpenExternal,
                intent: ResourceActionIntent::Standard,
                availability: ResourceActionAvailability::External,
                limitation: Some(super::CapabilityLimitation {
                    code: "external_resource".into(),
                    message_key: "agents.resources.externalResource".into(),
                }),
            },
        ],
    }
}

fn inspect_action() -> ResourceActionView {
    ResourceActionView {
        action: ResourceAction::Inspect,
        intent: ResourceActionIntent::Standard,
        availability: ResourceActionAvailability::Available,
        limitation: None,
    }
}

fn confirmation_action(action: ResourceAction) -> ResourceActionView {
    ResourceActionView {
        action,
        intent: ResourceActionIntent::Standard,
        availability: ResourceActionAvailability::ConfirmationRequired,
        limitation: None,
    }
}

fn unavailable_action(action: ResourceAction, code: &str, message_key: &str) -> ResourceActionView {
    ResourceActionView {
        action,
        intent: ResourceActionIntent::Standard,
        availability: ResourceActionAvailability::Unavailable,
        limitation: Some(super::CapabilityLimitation {
            code: code.into(),
            message_key: message_key.into(),
        }),
    }
}

pub(super) fn user_ownership_records_for(
    workspace: &UserWorkspaceDescriptor,
    kind: ResourceKind,
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
        if candidate.workspace_key != workspace.key || candidate.resource.kind != kind {
            continue;
        }
        if let Some(record) = load_ownership_record(&state, &candidate.resource)? {
            records.push(record);
        }
    }
    Ok(records)
}

fn validate_skill_record(record: &ResourceOwnershipRecord) -> Result<(), AgentError> {
    let target = Path::new(&record.target_path);
    let metadata = std::fs::symlink_metadata(target).map_err(|error| {
        inventory_record_error(
            record,
            format!("Owned Skill target is unavailable: {error}"),
        )
    })?;
    if !metadata.file_type().is_symlink() {
        return Err(inventory_record_error(
            record,
            "Owned Skill target is not a symlink",
        ));
    }
    let link = std::fs::read_link(target)
        .map_err(|error| inventory_record_error(record, error.to_string()))?;
    let digest = ContentDigest::sha256(link.to_string_lossy().as_bytes());
    validate_ownership_record(
        record,
        &record.resource,
        target,
        ResourceStateKind::Symlink,
        Some(&digest),
    )?;
    validate_ownership_artifact(record)
}

fn inventory_revision(
    workspace: &UserWorkspaceDescriptor,
    skills: &CollectionResourceInventory,
    plugins: &CollectionResourceInventory,
) -> Result<InventoryRevision, AgentError> {
    #[derive(Serialize)]
    struct RevisionInput<'a> {
        workspace: &'a UserWorkspaceDescriptor,
        skills: &'a CollectionResourceInventory,
        plugins: &'a CollectionResourceInventory,
    }
    let bytes = serde_json::to_vec(&RevisionInput {
        workspace,
        skills,
        plugins,
    })
    .map_err(|error| inventory_error(workspace, error.to_string()))?;
    let digest = ContentDigest::sha256(&bytes);
    Ok(InventoryRevision::from(opaque_contract_id(
        "user-inventory-revision",
        &[workspace.key.as_str(), digest.as_str()],
    )))
}

fn inventory_record_error(
    record: &ResourceOwnershipRecord,
    message: impl Into<String>,
) -> AgentError {
    AgentError {
        code: AgentErrorCode::PermissionDenied,
        message: message.into(),
        agent_id: None,
        installation_id: Some(record.resource.installation_id.clone()),
        resource: Some(record.resource.clone()),
        retryable: false,
        details: Some(serde_json::json!({"phase": "user_inventory"})),
    }
}

fn inventory_error(workspace: &UserWorkspaceDescriptor, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "user_inventory"})),
    }
}

pub(super) fn current_user_skill_digest(resource_id: &str) -> Result<ContentDigest, String> {
    let resolved =
        super::resolve_catalog_resource(resource_id).map_err(|error| error.to_string())?;
    if resolved.kind != ResourceKind::Skills {
        return Err("Catalog resource is not a Skill".into());
    }
    super::directory_tree_digest(&resolved.physical_path).map_err(|error| error.to_string())
}

pub(super) fn publish_user_skill_artifact(resource_id: &str) -> Result<PathBuf, String> {
    let catalog = load_resource_catalog_snapshot().map_err(|error| error.to_string())?;
    let resource = catalog
        .resources
        .get(resource_id)
        .filter(|resource| {
            resource.kind == ResourceKind::Skills
                && resource.present
                && resource.lifecycle == ResourceLifecycle::Managed
        })
        .ok_or_else(|| "Catalog Skill is unavailable".to_string())?;
    let skill_catalog = super::load_skill_catalog_snapshot().map_err(|error| error.to_string())?;
    let entry = skill_catalog
        .entries
        .iter()
        .find(|entry| entry.source_id == resource.source_id)
        .ok_or_else(|| "Catalog Skill source is unavailable".to_string())?;
    let physical_root = entry
        .current_binding
        .as_ref()
        .map(|binding| binding.physical_root.clone())
        .ok_or_else(|| "Catalog Skill source binding is unavailable".to_string())?;
    let source = crate::models::SkillSource {
        id: entry.source_id.clone(),
        source_type: SkillSourceType::Local,
        url: physical_root,
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: entry.added_at,
    };
    let staged = super::stage_skill_source(&source).map_err(|error| error.to_string())?;
    let artifact =
        super::publish_staged_skill_artifact(staged).map_err(|error| error.to_string())?;
    let item = artifact
        .skills
        .iter()
        .find(|item| item.logical_id == resource.install_id)
        .ok_or_else(|| "Catalog Skill artifact does not contain the resource".to_string())?;
    let tree = super::verify_skill_artifact(&artifact).map_err(|error| error.to_string())?;
    let path = tree.join(&item.subpath);
    if path.is_dir() && path.join("SKILL.md").is_file() {
        Ok(path)
    } else {
        Err("Catalog Skill artifact is invalid".into())
    }
}

#[cfg(test)]
mod user_plugin_management_tests {
    use super::*;

    #[test]
    fn partially_removed_managed_plugin_can_finish_removal() {
        let management = user_plugin_management(
            ResourceOwnershipKind::AdManaged,
            EffectiveResourceState::Unconfigured,
            true,
            true,
        );

        assert!(management.actions.iter().any(|action| {
            action.action == ResourceAction::Remove
                && action.availability == ResourceActionAvailability::ConfirmationRequired
        }));
    }
}

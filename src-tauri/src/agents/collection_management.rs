use super::{
    CapabilityLimitation, EffectiveResourceState, ResourceAction, ResourceActionAvailability,
    ResourceActionView, ResourceKind, ResourceManagementStatus, ResourceManagementView,
    ResourceOwnershipKind, WorkspaceDescriptor,
};

pub(super) struct CollectionManagementInput<'a> {
    pub workspace: &'a WorkspaceDescriptor,
    pub kind: ResourceKind,
    pub state: EffectiveResourceState,
    pub ownership: ResourceOwnershipKind,
    pub has_health_error: bool,
    pub owned_artifact: Option<&'a str>,
    pub available_artifact: Option<&'a str>,
    pub has_resettable_declaration: bool,
}

pub(super) fn resource_management(input: CollectionManagementInput<'_>) -> ResourceManagementView {
    let inspect = action(
        ResourceAction::Inspect,
        ResourceActionAvailability::Available,
    );
    if input.has_health_error || input.state == EffectiveResourceState::Conflict {
        return ResourceManagementView {
            status: ResourceManagementStatus::ReadOnly,
            actions: vec![inspect],
        };
    }
    if input.ownership == ResourceOwnershipKind::External {
        return ResourceManagementView {
            status: ResourceManagementStatus::External,
            actions: vec![
                inspect,
                limited_action(
                    ResourceAction::OpenExternal,
                    ResourceActionAvailability::External,
                    "external_resource",
                    "agents.resources.externalResource",
                ),
            ],
        };
    }
    match input.kind {
        ResourceKind::Skills if input.state == EffectiveResourceState::Unconfigured => {
            ResourceManagementView {
                status: ResourceManagementStatus::Managed,
                actions: vec![
                    inspect,
                    confirmation_action(ResourceAction::Install),
                    unavailable_action(
                        ResourceAction::Update,
                        "skill_not_installed",
                        "agents.resources.skillNotInstalled",
                    ),
                    unavailable_action(
                        ResourceAction::Remove,
                        "skill_not_installed",
                        "agents.resources.skillNotInstalled",
                    ),
                ],
            }
        }
        ResourceKind::Skills if input.owned_artifact.is_some() => {
            let update = match input.available_artifact {
                Some(artifact) if Some(artifact) != input.owned_artifact => {
                    confirmation_action(ResourceAction::Update)
                }
                Some(_) => unavailable_action(
                    ResourceAction::Update,
                    "skill_revision_current",
                    "agents.resources.skillRevisionCurrent",
                ),
                None => unavailable_action(
                    ResourceAction::Update,
                    "skill_source_unavailable",
                    "agents.resources.skillSourceUnavailable",
                ),
            };
            ResourceManagementView {
                status: ResourceManagementStatus::Managed,
                actions: vec![
                    inspect,
                    confirmation_action(if input.state == EffectiveResourceState::Enabled {
                        ResourceAction::Disable
                    } else {
                        ResourceAction::Enable
                    }),
                    update,
                    confirmation_action(ResourceAction::Remove),
                ],
            }
        }
        ResourceKind::Skills => ResourceManagementView {
            status: ResourceManagementStatus::ReadOnly,
            actions: vec![
                inspect,
                unavailable_action(
                    ResourceAction::Install,
                    "skill_ownership_unproven",
                    "agents.resources.skillOwnershipUnproven",
                ),
                unavailable_action(
                    ResourceAction::Remove,
                    "skill_ownership_unproven",
                    "agents.resources.skillOwnershipUnproven",
                ),
            ],
        },
        ResourceKind::Plugins if codex_runtime_unavailable(input.workspace) => {
            let toggle = if input.state == EffectiveResourceState::Enabled {
                ResourceAction::Disable
            } else {
                ResourceAction::Enable
            };
            ResourceManagementView {
                status: ResourceManagementStatus::ReadOnly,
                actions: vec![
                    inspect,
                    unavailable_action(
                        toggle,
                        "codex_runtime_not_prepared",
                        "agents.resources.codexRuntimeNotPrepared",
                    ),
                    unavailable_action(
                        ResourceAction::Remove,
                        "codex_runtime_not_prepared",
                        "agents.resources.codexRuntimeNotPrepared",
                    ),
                    unavailable_action(
                        ResourceAction::Update,
                        "plugin_update_requires_conversion_source",
                        "agents.resources.pluginUpdateRequiresConversionSource",
                    ),
                ],
            }
        }
        ResourceKind::Plugins => plugin_management(input, inspect),
        _ => ResourceManagementView {
            status: ResourceManagementStatus::Unsupported,
            actions: vec![inspect],
        },
    }
}

fn plugin_management(
    input: CollectionManagementInput<'_>,
    inspect: ResourceActionView,
) -> ResourceManagementView {
    let toggle = if input.state == EffectiveResourceState::Enabled {
        ResourceAction::Disable
    } else {
        ResourceAction::Enable
    };
    let codex = input.workspace.agent_id.as_str() == "codex";
    ResourceManagementView {
        status: ResourceManagementStatus::Managed,
        actions: vec![
            inspect,
            confirmation_action(toggle),
            if input.has_resettable_declaration {
                confirmation_action(ResourceAction::Remove)
            } else {
                unavailable_action(
                    ResourceAction::Remove,
                    "plugin_inherited_only",
                    "agents.resources.pluginInheritedOnly",
                )
            },
            unavailable_action(
                ResourceAction::Update,
                if codex {
                    "plugin_update_requires_conversion_source"
                } else {
                    "plugin_update_external"
                },
                if codex {
                    "agents.resources.pluginUpdateRequiresConversionSource"
                } else {
                    "agents.resources.pluginUpdateExternal"
                },
            ),
        ],
    }
}

fn codex_runtime_unavailable(workspace: &WorkspaceDescriptor) -> bool {
    workspace.agent_id.as_str() == "codex" && workspace.project_runtime.is_none()
}

fn confirmation_action(action: ResourceAction) -> ResourceActionView {
    self::action(action, ResourceActionAvailability::ConfirmationRequired)
}

fn unavailable_action(action: ResourceAction, code: &str, message_key: &str) -> ResourceActionView {
    limited_action(
        action,
        ResourceActionAvailability::Unavailable,
        code,
        message_key,
    )
}

fn action(action: ResourceAction, availability: ResourceActionAvailability) -> ResourceActionView {
    ResourceActionView {
        action,
        availability,
        limitation: None,
    }
}

fn limited_action(
    action: ResourceAction,
    availability: ResourceActionAvailability,
    code: &str,
    message_key: &str,
) -> ResourceActionView {
    ResourceActionView {
        action,
        availability,
        limitation: Some(CapabilityLimitation {
            code: code.into(),
            message_key: message_key.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentInstallation;

    #[test]
    fn unprepared_codex_runtime_exposes_no_project_plugin_mutation() {
        let installation = AgentInstallation::with_id("codex:test", "codex", "/Users/test/.codex");
        let workspace =
            WorkspaceDescriptor::for_installation("/Users/test/project", &installation, None);

        let management = resource_management(CollectionManagementInput {
            workspace: &workspace,
            kind: ResourceKind::Plugins,
            state: EffectiveResourceState::Enabled,
            ownership: ResourceOwnershipKind::AgentManaged,
            has_health_error: false,
            owned_artifact: None,
            available_artifact: None,
            has_resettable_declaration: true,
        });

        assert_eq!(management.status, ResourceManagementStatus::ReadOnly);
        assert!(management.actions.iter().any(|action| {
            action.action == ResourceAction::Disable
                && action.availability == ResourceActionAvailability::Unavailable
                && action
                    .limitation
                    .as_ref()
                    .is_some_and(|limitation| limitation.code == "codex_runtime_not_prepared")
        }));
    }
}

use super::{
    CapabilityLimitation, EffectiveResourceState, ResourceAction, ResourceActionAvailability,
    ResourceActionIntent, ResourceActionView, ResourceKind, ResourceManagementStatus,
    ResourceManagementView, ResourceOwnershipKind, WorkspaceDescriptor,
};

pub(super) struct CollectionManagementInput<'a> {
    pub workspace: &'a WorkspaceDescriptor,
    pub kind: ResourceKind,
    pub state: EffectiveResourceState,
    pub ownership: ResourceOwnershipKind,
    pub agent_supported: bool,
    pub target_occupied: bool,
    pub has_health_error: bool,
    pub owned_artifact: Option<&'a str>,
    pub owned_source_binding: bool,
    pub available_artifact: Option<&'a str>,
    pub has_resettable_declaration: bool,
    pub has_user_declaration: bool,
    pub owned_scope: Option<super::ResourceScope>,
}

pub(super) fn resource_management(input: CollectionManagementInput<'_>) -> ResourceManagementView {
    let inspect = action(
        ResourceAction::Inspect,
        ResourceActionAvailability::Available,
    );
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
    if input.has_user_declaration
        && input.state != EffectiveResourceState::Unconfigured
        && input.owned_scope != Some(super::ResourceScope::Project)
    {
        let toggle = if input.state == EffectiveResourceState::Enabled {
            ResourceAction::Disable
        } else {
            ResourceAction::Enable
        };
        let runtime_unavailable = codex_runtime_unavailable(input.workspace);
        return ResourceManagementView {
            status: if runtime_unavailable {
                ResourceManagementStatus::ReadOnly
            } else if input.ownership == ResourceOwnershipKind::External {
                ResourceManagementStatus::External
            } else {
                ResourceManagementStatus::Managed
            },
            actions: vec![
                inspect,
                if runtime_unavailable {
                    unavailable_action(
                        toggle,
                        "codex_runtime_not_prepared",
                        "agents.resources.codexRuntimeNotPrepared",
                    )
                } else {
                    confirmation_action(toggle)
                },
                unavailable_action(
                    ResourceAction::Remove,
                    "resource_inherited_from_user",
                    "agents.resources.inheritedFromUser",
                ),
                unavailable_action(
                    ResourceAction::Update,
                    "resource_inherited_from_user",
                    "agents.resources.inheritedFromUser",
                ),
            ],
        };
    }
    if input.has_health_error || input.state == EffectiveResourceState::Conflict {
        let mut actions = vec![inspect];
        if input.has_health_error
            && input.kind == ResourceKind::Skills
            && input.owned_source_binding
        {
            actions.push(limited_action_with_intent(
                ResourceAction::Update,
                ResourceActionIntent::Repair,
                ResourceActionAvailability::Unavailable,
                "skill_source_unavailable",
                "agents.resources.skillSourceUnavailable",
            ));
        }
        return ResourceManagementView {
            status: ResourceManagementStatus::ReadOnly,
            actions,
        };
    }
    match input.kind {
        ResourceKind::Skills if input.state == EffectiveResourceState::Unconfigured => {
            ResourceManagementView {
                status: ResourceManagementStatus::Managed,
                actions: vec![
                    inspect,
                    if input.target_occupied {
                        unavailable_action(
                            ResourceAction::Install,
                            "target_occupied",
                            "agents.resources.targetOccupied",
                        )
                    } else {
                        confirmation_action(ResourceAction::Install)
                    },
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
                    confirmation_action_with_intent(
                        ResourceAction::Update,
                        if input.owned_source_binding {
                            ResourceActionIntent::Standard
                        } else {
                            ResourceActionIntent::Relink
                        },
                    )
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
        ResourceKind::Plugins if input.state == EffectiveResourceState::Unconfigured => {
            let install = if !input.agent_supported {
                unavailable_action(
                    ResourceAction::Install,
                    "unsupported_agent_capability",
                    "agents.resources.unsupportedAgentCapability",
                )
            } else if input.target_occupied {
                unavailable_action(
                    ResourceAction::Install,
                    "target_occupied",
                    "agents.resources.targetOccupied",
                )
            } else {
                confirmation_action(ResourceAction::Install)
            };
            ResourceManagementView {
                status: ResourceManagementStatus::Managed,
                actions: vec![inspect, install],
            }
        }
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
                        "unsupported_agent_capability",
                        "agents.resources.unsupportedAgentCapability",
                    ),
                ],
            }
        }
        ResourceKind::Plugins if input.owned_artifact.is_some() => {
            let toggle = if input.state == EffectiveResourceState::Enabled {
                ResourceAction::Disable
            } else {
                ResourceAction::Enable
            };
            ResourceManagementView {
                status: ResourceManagementStatus::Managed,
                actions: vec![
                    inspect,
                    confirmation_action(toggle),
                    confirmation_action(ResourceAction::Remove),
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
                    "unsupported_agent_capability"
                } else {
                    "plugin_update_external"
                },
                if codex {
                    "agents.resources.unsupportedAgentCapability"
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

fn confirmation_action_with_intent(
    action: ResourceAction,
    intent: ResourceActionIntent,
) -> ResourceActionView {
    ResourceActionView {
        action,
        intent,
        availability: ResourceActionAvailability::ConfirmationRequired,
        limitation: None,
    }
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
        intent: ResourceActionIntent::Standard,
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
    limited_action_with_intent(
        action,
        ResourceActionIntent::Standard,
        availability,
        code,
        message_key,
    )
}

fn limited_action_with_intent(
    action: ResourceAction,
    intent: ResourceActionIntent,
    availability: ResourceActionAvailability,
    code: &str,
    message_key: &str,
) -> ResourceActionView {
    ResourceActionView {
        action,
        intent,
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
            agent_supported: false,
            target_occupied: false,
            has_health_error: false,
            owned_artifact: None,
            owned_source_binding: false,
            available_artifact: None,
            has_resettable_declaration: true,
            has_user_declaration: false,
            owned_scope: None,
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

    #[test]
    fn inherited_external_resource_never_exposes_a_project_toggle() {
        let installation =
            AgentInstallation::with_id("claude:test", "claude-code", "/Users/test/.claude");
        let workspace =
            WorkspaceDescriptor::for_installation("/Users/test/project", &installation, None);

        for kind in [ResourceKind::Skills, ResourceKind::Plugins] {
            let management = resource_management(CollectionManagementInput {
                workspace: &workspace,
                kind,
                state: EffectiveResourceState::Enabled,
                ownership: ResourceOwnershipKind::External,
                agent_supported: true,
                target_occupied: true,
                has_health_error: false,
                owned_artifact: None,
                owned_source_binding: false,
                available_artifact: None,
                has_resettable_declaration: false,
                has_user_declaration: true,
                owned_scope: None,
            });

            assert_eq!(management.status, ResourceManagementStatus::External);
            assert!(!management.actions.iter().any(|action| matches!(
                action.action,
                ResourceAction::Enable
                    | ResourceAction::Disable
                    | ResourceAction::Update
                    | ResourceAction::Remove
            )));
        }
    }

    #[test]
    fn unconfigured_codex_plugin_exposes_unsupported_install() {
        let installation = AgentInstallation::with_id("codex:test", "codex", "/Users/test/.codex");
        let workspace =
            WorkspaceDescriptor::for_installation("/Users/test/project", &installation, None);

        let management = resource_management(CollectionManagementInput {
            workspace: &workspace,
            kind: ResourceKind::Plugins,
            state: EffectiveResourceState::Unconfigured,
            ownership: ResourceOwnershipKind::AdManaged,
            agent_supported: false,
            target_occupied: false,
            has_health_error: false,
            owned_artifact: None,
            owned_source_binding: false,
            available_artifact: None,
            has_resettable_declaration: false,
            has_user_declaration: false,
            owned_scope: None,
        });

        assert_eq!(management.status, ResourceManagementStatus::Managed);
        assert!(management.actions.iter().any(|action| {
            action.action == ResourceAction::Install
                && action.availability == ResourceActionAvailability::Unavailable
                && action
                    .limitation
                    .as_ref()
                    .is_some_and(|limitation| limitation.code == "unsupported_agent_capability")
        }));
    }

    #[test]
    fn legacy_skill_update_is_exposed_as_relink() {
        let installation =
            AgentInstallation::with_id("claude:test", "claude-code", "/Users/test/.claude");
        let workspace =
            WorkspaceDescriptor::for_installation("/Users/test/project", &installation, None);

        let management = resource_management(CollectionManagementInput {
            workspace: &workspace,
            kind: ResourceKind::Skills,
            state: EffectiveResourceState::Enabled,
            ownership: ResourceOwnershipKind::AdManaged,
            agent_supported: true,
            target_occupied: false,
            has_health_error: false,
            owned_artifact: Some("/Users/test/.ad/artifacts/skills/old/tree/review"),
            owned_source_binding: false,
            available_artifact: Some("/Users/test/source/review"),
            has_resettable_declaration: false,
            has_user_declaration: false,
            owned_scope: Some(super::super::ResourceScope::Project),
        });

        assert!(management.actions.iter().any(|action| {
            action.action == ResourceAction::Update
                && action.intent == ResourceActionIntent::Relink
                && action.availability == ResourceActionAvailability::ConfirmationRequired
        }));
    }

    #[test]
    fn broken_live_skill_binding_exposes_unavailable_repair() {
        let installation =
            AgentInstallation::with_id("claude:test", "claude-code", "/Users/test/.claude");
        let workspace =
            WorkspaceDescriptor::for_installation("/Users/test/project", &installation, None);

        let management = resource_management(CollectionManagementInput {
            workspace: &workspace,
            kind: ResourceKind::Skills,
            state: EffectiveResourceState::Enabled,
            ownership: ResourceOwnershipKind::AdManaged,
            agent_supported: true,
            target_occupied: false,
            has_health_error: true,
            owned_artifact: Some("/Users/test/source/review"),
            owned_source_binding: true,
            available_artifact: None,
            has_resettable_declaration: false,
            has_user_declaration: false,
            owned_scope: Some(super::super::ResourceScope::Project),
        });

        assert!(management.actions.iter().any(|action| {
            action.action == ResourceAction::Update
                && action.intent == ResourceActionIntent::Repair
                && action.availability == ResourceActionAvailability::Unavailable
        }));
    }
}

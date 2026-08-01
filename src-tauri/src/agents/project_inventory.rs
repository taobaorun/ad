use std::path::Path;

use serde::Serialize;

use super::collection_inventory::{failed_collection, inspect_plugins, inspect_skills};
use super::settings_inventory::{failed_settings_view, inspect_effective_settings};
use super::{
    builtin_registry, opaque_contract_id, resolve_project_agent_workspace,
    AdapterDiscoveryContract, AgentError, AgentErrorCode, CollectionResourceInventory,
    ContentDigest, CoverageStatus, DiscoveryCompatibility, InstallationId, InventoryRevision,
    ItemDiagnostic, ProjectWorkspaceInventory, ResourceKind, WorkspaceDescriptor,
};

pub fn inspect_project_workspace_inventory(
    installation_id: &InstallationId,
    project_path: &Path,
) -> Result<ProjectWorkspaceInventory, AgentError> {
    let workspace = resolve_project_agent_workspace(installation_id, project_path)?;
    let discovery = discovery_contract(&workspace)?;
    let version_diagnostic = ItemDiagnostic {
        code: "agent_version_unverified".into(),
        message_key: "agents.inventory.agentVersionUnverified".into(),
        retryable: false,
        resource_key: None,
    };
    let settings_result = inspect_effective_settings(&workspace, &version_diagnostic);
    let (settings, settings_layers, runtime_manifest, settings_revision, mut diagnostics) =
        match settings_result {
            Ok(inspection) => (
                inspection.view,
                inspection.layers,
                inspection.runtime_manifest,
                inspection.private_revision,
                Vec::new(),
            ),
            Err(error) => {
                let diagnostic = diagnostic_from_error("settings_inventory_failed", &error);
                (
                    failed_settings_view(&workspace, diagnostic.clone()),
                    Vec::new(),
                    None,
                    ContentDigest::sha256(error.message.as_bytes()),
                    vec![diagnostic],
                )
            }
        };

    let skills = inspect_skills(&workspace, &version_diagnostic).unwrap_or_else(|error| {
        let diagnostic = diagnostic_from_error("skills_inventory_failed", &error);
        diagnostics.push(diagnostic.clone());
        failed_collection(&workspace, ResourceKind::Skills, diagnostic)
    });
    let plugins = if settings.coverage.status == CoverageStatus::Failed {
        let diagnostic = ItemDiagnostic {
            code: "plugins_settings_dependency_failed".into(),
            message_key: "agents.inventory.pluginsSettingsDependencyFailed".into(),
            retryable: false,
            resource_key: None,
        };
        diagnostics.push(diagnostic.clone());
        failed_collection(&workspace, ResourceKind::Plugins, diagnostic)
    } else {
        inspect_plugins(
            &workspace,
            &settings_layers,
            runtime_manifest.as_ref(),
            &version_diagnostic,
        )
        .unwrap_or_else(|error| {
            let diagnostic = diagnostic_from_error("plugins_inventory_failed", &error);
            diagnostics.push(diagnostic.clone());
            failed_collection(&workspace, ResourceKind::Plugins, diagnostic)
        })
    };
    diagnostics.push(version_diagnostic);
    let revision = inventory_revision(
        &workspace,
        &discovery,
        &settings_revision,
        &settings,
        &skills,
        &plugins,
    )?;

    Ok(ProjectWorkspaceInventory {
        schema_version: 1,
        workspace,
        revision,
        discovery,
        settings,
        skills,
        plugins,
        diagnostics,
    })
}

fn discovery_contract(
    workspace: &WorkspaceDescriptor,
) -> Result<AdapterDiscoveryContract, AgentError> {
    let registry = builtin_registry();
    let definition = registry
        .adapter(workspace.agent_id.as_str())
        .map(|adapter| adapter.definition())
        .ok_or_else(|| inventory_error(workspace, "Unknown Agent adapter"))?;
    let (location_set, schema_versions) = match workspace.agent_id.as_str() {
        "claude-code" => (
            "claude-project-v1",
            vec!["claude-settings-json-v1", "claude-skills-directory-v1"],
        ),
        "codex" => (
            "codex-project-v1",
            vec![
                "codex-config-toml-v1",
                "agents-skills-directory-v1",
                "ad-runtime-manifest-v1",
            ],
        ),
        _ => ("unknown-project-v1", Vec::new()),
    };
    Ok(AdapterDiscoveryContract {
        adapter_version: definition.adapter_version,
        location_set: location_set.into(),
        schema_versions: schema_versions.into_iter().map(str::to_owned).collect(),
        observed_agent_version: None,
        verified_agent_versions: Vec::new(),
        compatibility: DiscoveryCompatibility::Unverified,
    })
}

fn diagnostic_from_error(code: &str, error: &AgentError) -> ItemDiagnostic {
    ItemDiagnostic {
        code: code.into(),
        message_key: format!("agents.inventory.{code}"),
        retryable: error.retryable,
        resource_key: None,
    }
}

fn inventory_revision(
    workspace: &WorkspaceDescriptor,
    discovery: &AdapterDiscoveryContract,
    private_settings_revision: &ContentDigest,
    settings: &super::SettingsEffectiveView,
    skills: &CollectionResourceInventory,
    plugins: &CollectionResourceInventory,
) -> Result<InventoryRevision, AgentError> {
    #[derive(Serialize)]
    struct RevisionInput<'a> {
        workspace_revision: &'a super::WorkspaceRevision,
        private_settings_revision: &'a ContentDigest,
        discovery: &'a AdapterDiscoveryContract,
        settings: &'a super::SettingsEffectiveView,
        skills: &'a CollectionResourceInventory,
        plugins: &'a CollectionResourceInventory,
    }
    let bytes = serde_json::to_vec(&RevisionInput {
        workspace_revision: &workspace.revision,
        private_settings_revision,
        discovery,
        settings,
        skills,
        plugins,
    })
    .map_err(|error| inventory_error(workspace, error.to_string()))?;
    let digest = ContentDigest::sha256(&bytes);
    Ok(InventoryRevision::from(opaque_contract_id(
        "inventory-revision",
        &[workspace.key.as_str(), digest.as_str()],
    )))
}

fn inventory_error(workspace: &WorkspaceDescriptor, message: impl Into<String>) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.effective_installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "project_inventory"})),
    }
}

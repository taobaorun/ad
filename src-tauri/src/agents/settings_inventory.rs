use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    builtin_registry, load_project_codex_runtime_manifest, project_runtime_for_base_project,
    AgentContext, AgentError, CategoryCoverage, ContentDigest, CoverageStatus, DeclarationKey,
    ItemDiagnostic, PhysicalTargetId, ProjectCodexRuntimeManifest, ResourceDeclarationView,
    ResourceKey, ResourceKind, ResourceLayer, ResourceRef, ResourceScope, SettingsDocument,
    SettingsEditableTargetView, SettingsEffectiveView, SettingsFieldDeclarationView,
    SettingsFieldView, SettingsLayerView, SettingsValueSensitivity, WorkspaceDescriptor,
};

pub(super) const MASKED_SETTINGS_VALUE: &str = "••••••••";

pub(super) struct SettingsInspection {
    pub view: SettingsEffectiveView,
    pub layers: Vec<SettingsLayerSemantic>,
    pub runtime_manifest: Option<ProjectCodexRuntimeManifest>,
    pub private_revision: ContentDigest,
}

#[derive(Clone)]
pub(super) struct SettingsLayerSemantic {
    pub declaration_key: DeclarationKey,
    pub logical_id: String,
    pub layer: ResourceLayer,
    pub content: Value,
}

pub(super) fn inspect_effective_settings(
    workspace: &WorkspaceDescriptor,
    version_diagnostic: &ItemDiagnostic,
) -> Result<SettingsInspection, AgentError> {
    let registry = builtin_registry();
    let adapter = registry
        .adapter(workspace.agent_id.as_str())
        .ok_or_else(|| {
            settings_error(
                workspace,
                format!("Unknown Agent adapter: {}", workspace.agent_id),
            )
        })?;
    let settings = adapter.settings().ok_or_else(|| {
        settings_error(
            workspace,
            format!("Agent {} does not support Settings", workspace.agent_id),
        )
    })?;
    let base_context = AgentContext {
        installation_id: workspace.base_installation_id.clone(),
        project_path: Some(workspace.canonical_project_path.clone()),
    };
    let documents = if workspace.agent_id.as_str() == "claude-code" {
        settings.edit_documents(&base_context)?
    } else {
        settings
            .inspect(&base_context)?
            .into_iter()
            .map(SettingsDocument::from)
            .collect()
    };
    let mut inputs = documents
        .into_iter()
        .map(|document| document_input(workspace, document))
        .collect::<Result<Vec<_>, _>>()?;
    let runtime_manifest = if workspace.agent_id.as_str() == "codex" {
        append_codex_runtime_layer(workspace, &mut inputs)?
    } else {
        None
    };
    inputs.sort_by_key(|input| layer_priority(input.layer, &input.logical_id));
    let private_revision = ContentDigest::sha256(
        &serde_json::to_vec(&inputs)
            .map_err(|error| settings_error(workspace, error.to_string()))?,
    );

    let settings_key = ResourceKey::for_collection(
        &workspace.key,
        &workspace.agent_id,
        ResourceKind::Settings,
        "effective-settings",
        "agent-settings",
    );
    let mut effective = Value::Object(Map::new());
    let mut field_declarations = BTreeMap::<String, Vec<SettingsFieldDeclarationView>>::new();
    let mut layers = Vec::new();
    let mut semantics = Vec::new();
    let mut editable_targets = Vec::new();
    let mut observed = 0usize;

    for input in inputs {
        let declaration_key =
            DeclarationKey::for_layer(&settings_key, input.layer, input.source_id.as_str());
        let target_id = PhysicalTargetId::for_resource(&input.resource);
        let mut redacted_paths = BTreeSet::new();
        let masked_content = mask_settings_value(&input.content, "", &mut redacted_paths);
        if input.exists {
            observed += 1;
            merge_semantic_value(&mut effective, &input.content);
            let mut flattened = BTreeMap::new();
            flatten_settings_fields(&input.content, "", &mut flattened);
            for (path, value) in flattened {
                let sensitivity = sensitivity_for_path(&path);
                field_declarations
                    .entry(path)
                    .or_default()
                    .push(SettingsFieldDeclarationView {
                        declaration_key: declaration_key.clone(),
                        layer: input.layer,
                        value: mask_leaf(value, sensitivity),
                        sensitivity,
                    });
            }
        }
        let declaration = ResourceDeclarationView {
            key: declaration_key.clone(),
            layer: input.layer,
            source_id: input.source_id.clone(),
            target_id,
            scope: Some(input.resource.scope),
        };
        layers.push(SettingsLayerView {
            declaration,
            logical_id: input.logical_id.clone(),
            media_type: input.media_type.clone(),
            content: masked_content,
            exists: input.exists,
            editable: input.editable,
            preserves_unknown_fields: true,
            redacted_paths: redacted_paths.iter().cloned().collect(),
        });
        if input.editable {
            editable_targets.push(SettingsEditableTargetView {
                declaration_key: declaration_key.clone(),
                resource: input.resource.clone(),
                media_type: input.media_type.clone(),
                exists: input.exists,
                preserves_unknown_fields: true,
                redacted_paths: redacted_paths.into_iter().collect(),
            });
        }
        semantics.push(SettingsLayerSemantic {
            declaration_key,
            logical_id: input.logical_id,
            layer: input.layer,
            content: input.content,
        });
    }

    let fields = field_declarations
        .into_iter()
        .filter_map(|(path, declarations)| {
            let winner = declarations.last()?;
            Some(SettingsFieldView {
                path,
                value: winner.value.clone(),
                sensitivity: winner.sensitivity,
                winner: winner.declaration_key.clone(),
                declarations,
            })
        })
        .collect::<Vec<_>>();
    let mut effective_redactions = BTreeSet::new();
    let effective_content = mask_settings_value(&effective, "", &mut effective_redactions);
    let coverage = CategoryCoverage {
        status: CoverageStatus::Partial,
        observed,
        visible: fields.len(),
        diagnostics: vec![version_diagnostic.clone()],
    };

    Ok(SettingsInspection {
        view: SettingsEffectiveView {
            workspace_key: workspace.key.clone(),
            coverage,
            effective_content,
            fields,
            layers,
            editable_targets,
        },
        layers: semantics,
        runtime_manifest,
        private_revision,
    })
}

#[derive(Serialize)]
struct SettingsLayerInput {
    resource: ResourceRef,
    logical_id: String,
    layer: ResourceLayer,
    source_id: String,
    media_type: String,
    content: Value,
    exists: bool,
    editable: bool,
}

fn document_input(
    workspace: &WorkspaceDescriptor,
    document: SettingsDocument,
) -> Result<SettingsLayerInput, AgentError> {
    let logical_id = document.resource.logical_id.clone();
    let layer = settings_layer(workspace.agent_id.as_str(), &document.resource, &logical_id);
    let content = semantic_content(workspace, &document.media_type, document.content)?;
    let editable = workspace.agent_id.as_str() == "claude-code"
        && document.resource.scope == ResourceScope::Project;
    Ok(SettingsLayerInput {
        resource: document.resource,
        source_id: logical_id.clone(),
        logical_id,
        layer,
        media_type: document.media_type,
        content,
        exists: document.exists,
        editable,
    })
}

fn append_codex_runtime_layer(
    workspace: &WorkspaceDescriptor,
    inputs: &mut Vec<SettingsLayerInput>,
) -> Result<Option<ProjectCodexRuntimeManifest>, AgentError> {
    if workspace.project_runtime.is_none() {
        return Ok(None);
    }
    let runtime = project_runtime_for_base_project(
        &workspace.base_installation_id,
        std::path::Path::new(&workspace.canonical_project_path),
    )
    .ok_or_else(|| settings_error(workspace, "Prepared Project Runtime state is missing"))?;
    let manifest = load_project_codex_runtime_manifest(&runtime)
        .map_err(|error| settings_error(workspace, error.to_string()))?
        .ok_or_else(|| settings_error(workspace, "Prepared Project Runtime manifest is missing"))?
        .manifest;
    let generated = std::fs::read_to_string(runtime.runtime_home.join("config.toml"))
        .map_err(|error| {
            settings_error(workspace, format!("Failed to read runtime config: {error}"))
        })?
        .parse::<toml::Value>()
        .map_err(|error| settings_error(workspace, format!("Invalid runtime config: {error}")))?;
    let generated = generated
        .as_table()
        .ok_or_else(|| settings_error(workspace, "Project Runtime config must be a TOML table"))?;
    let mut project_settings = Map::new();
    for key in &manifest.project_settings_keys {
        let value = generated.get(key).ok_or_else(|| {
            settings_error(
                workspace,
                format!("Project Runtime setting {key} is missing from materialized config"),
            )
        })?;
        project_settings.insert(
            key.clone(),
            serde_json::to_value(value)
                .map_err(|error| settings_error(workspace, error.to_string()))?,
        );
    }
    inputs.push(SettingsLayerInput {
        resource: ResourceRef {
            installation_id: workspace.effective_installation_id.clone(),
            project_path: Some(workspace.canonical_project_path.clone()),
            kind: ResourceKind::Settings,
            scope: ResourceScope::Project,
            logical_id: "runtime-config".into(),
        },
        logical_id: "runtime-config".into(),
        layer: ResourceLayer::Runtime,
        source_id: "runtime-manifest".into(),
        media_type: "application/vnd.ad.project-settings+json".into(),
        content: Value::Object(project_settings),
        exists: true,
        editable: true,
    });
    Ok(Some(manifest))
}

fn settings_layer(agent_id: &str, resource: &ResourceRef, logical_id: &str) -> ResourceLayer {
    if resource.scope == ResourceScope::User {
        return ResourceLayer::User;
    }
    if agent_id == "claude-code" && logical_id == "project-local" {
        ResourceLayer::Runtime
    } else {
        ResourceLayer::Project
    }
}

fn layer_priority(layer: ResourceLayer, logical_id: &str) -> u8 {
    match (layer, logical_id) {
        (ResourceLayer::System, _) => 0,
        (ResourceLayer::User, _) => 1,
        (ResourceLayer::Project, "project-shared") => 2,
        (ResourceLayer::Project, _) => 3,
        (ResourceLayer::Runtime, _) => 4,
    }
}

fn semantic_content(
    workspace: &WorkspaceDescriptor,
    media_type: &str,
    content: Value,
) -> Result<Value, AgentError> {
    match media_type {
        "application/json" if content.is_object() => Ok(content),
        "application/toml" => {
            let text = content
                .as_str()
                .ok_or_else(|| settings_error(workspace, "TOML settings must be text"))?;
            let value = text
                .parse::<toml::Value>()
                .map_err(|error| settings_error(workspace, error.to_string()))?;
            serde_json::to_value(value)
                .map_err(|error| settings_error(workspace, error.to_string()))
        }
        _ if content.is_object() => Ok(content),
        _ => Err(settings_error(
            workspace,
            format!("Unsupported Settings media type: {media_type}"),
        )),
    }
}

fn merge_semantic_value(target: &mut Value, incoming: &Value) {
    match (target, incoming) {
        (Value::Object(target), Value::Object(incoming)) => {
            for (key, value) in incoming {
                match target.get_mut(key) {
                    Some(existing) => merge_semantic_value(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, incoming) => *target = incoming.clone(),
    }
}

fn flatten_settings_fields(value: &Value, path: &str, output: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(values) if !values.is_empty() => {
            for (key, value) in values {
                let path = format!("{}/{}", path, json_pointer_segment(key));
                flatten_settings_fields(value, &path, output);
            }
        }
        _ if !path.is_empty() => {
            output.insert(path.to_owned(), value.clone());
        }
        _ => {}
    }
}

fn mask_settings_value(value: &Value, path: &str, redactions: &mut BTreeSet<String>) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    let child_path = format!("{}/{}", path, json_pointer_segment(key));
                    (
                        key.clone(),
                        mask_settings_value(value, &child_path, redactions),
                    )
                })
                .collect(),
        ),
        Value::Array(values) if sensitivity_for_path(path) == SettingsValueSensitivity::Public => {
            Value::Array(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        mask_settings_value(value, &format!("{path}/{index}"), redactions)
                    })
                    .collect(),
            )
        }
        _ => {
            let sensitivity = sensitivity_for_path(path);
            if sensitivity == SettingsValueSensitivity::Sensitive {
                redactions.insert(path.to_owned());
            }
            mask_leaf(value.clone(), sensitivity)
        }
    }
}

fn mask_leaf(value: Value, sensitivity: SettingsValueSensitivity) -> Value {
    if sensitivity == SettingsValueSensitivity::Sensitive && !value.is_null() {
        Value::String(MASKED_SETTINGS_VALUE.into())
    } else {
        value
    }
}

pub(super) fn restore_masked_settings_values(proposed: &mut Value, current: &Value) {
    if proposed.as_str() == Some(MASKED_SETTINGS_VALUE) {
        *proposed = current.clone();
        return;
    }
    match (proposed, current) {
        (Value::Object(proposed), Value::Object(current)) => {
            for (key, value) in proposed {
                if let Some(current) = current.get(key) {
                    restore_masked_settings_values(value, current);
                }
            }
        }
        (Value::Array(proposed), Value::Array(current)) => {
            for (index, value) in proposed.iter_mut().enumerate() {
                if let Some(current) = current.get(index) {
                    restore_masked_settings_values(value, current);
                }
            }
        }
        _ => {}
    }
}

fn sensitivity_for_path(path: &str) -> SettingsValueSensitivity {
    let normalized = path.to_ascii_lowercase().replace(['-', '_'], "");
    let sensitive = [
        "token",
        "secret",
        "password",
        "credential",
        "apikey",
        "accesskey",
        "privatekey",
        "authorization",
        "bearer",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
        || (normalized.contains("mcpservers") && normalized.contains("/env/"));
    if sensitive {
        SettingsValueSensitivity::Sensitive
    } else {
        SettingsValueSensitivity::Public
    }
}

fn json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn settings_error(workspace: &WorkspaceDescriptor, message: impl Into<String>) -> AgentError {
    AgentError {
        code: super::AgentErrorCode::InvalidPlan,
        message: message.into(),
        agent_id: Some(workspace.agent_id.clone()),
        installation_id: Some(workspace.effective_installation_id.clone()),
        resource: None,
        retryable: false,
        details: Some(serde_json::json!({"phase": "settings_inventory"})),
    }
}

pub(super) fn failed_settings_view(
    workspace: &WorkspaceDescriptor,
    diagnostic: ItemDiagnostic,
) -> SettingsEffectiveView {
    SettingsEffectiveView {
        workspace_key: workspace.key.clone(),
        coverage: CategoryCoverage {
            status: CoverageStatus::Failed,
            observed: 0,
            visible: 0,
            diagnostics: vec![diagnostic],
        },
        effective_content: Value::Object(Map::new()),
        fields: Vec::new(),
        layers: Vec::new(),
        editable_targets: Vec::new(),
    }
}

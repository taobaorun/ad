use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::execution_state::{ExecutionState, StateDirectory};
use super::{
    load_skill_catalog_snapshot, opaque_contract_id, verify_skill_artifact, ContentDigest,
    ResourceKind, ResourceRef, SkillCatalogEntry, SkillCatalogError, SkillCatalogSnapshot,
    SkillSourceType, SourceResourceItem, TargetLockSet,
};

pub const RESOURCE_CATALOG_SCHEMA_VERSION: u32 = 1;
const RESOURCE_CATALOG_FILE: &str = "resource_catalog.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLifecycle {
    Managed,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSourceHealth {
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogSourceBinding {
    pub binding_id: String,
    pub source_revision: String,
    pub stable_root: String,
    pub physical_root: String,
    pub tree_digest: ContentDigest,
    pub manifest_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogSource {
    pub id: String,
    pub display_name: String,
    pub source_type: SkillSourceType,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<CatalogSourceBinding>,
    pub health: CatalogSourceHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogResource {
    pub id: String,
    pub source_id: String,
    pub kind: ResourceKind,
    pub install_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Read from the current descriptor for display, never cached in the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub subpath: String,
    pub descriptor_digest: ContentDigest,
    #[serde(default)]
    pub compatible_agents: BTreeSet<String>,
    pub present: bool,
    pub lifecycle: ResourceLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressed_at_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_diagnostic_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceCatalogSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub sources: BTreeMap<String, CatalogSource>,
    pub resources: BTreeMap<String, CatalogResource>,
    #[serde(default)]
    pub missing_installations_by_source: BTreeMap<String, usize>,
    pub migrated_from_skill_catalog: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResourceCatalogDocument {
    pub(super) schema_version: u32,
    pub(super) revision: u64,
    pub(super) sources: BTreeMap<String, CatalogSource>,
    pub(super) resources: BTreeMap<String, CatalogResource>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceCatalogError {
    #[error("resource catalog is corrupt: {0}")]
    Corrupt(String),
    #[error("resource catalog resource was not found: {0}")]
    NotFound(String),
    #[error("resource catalog resource is not available: {0}")]
    Unavailable(String),
    #[error("resource catalog I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Legacy(#[from] SkillCatalogError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCatalogResource {
    pub resource_id: String,
    pub source_id: String,
    pub kind: ResourceKind,
    pub install_id: String,
    pub stable_path: PathBuf,
    pub physical_path: PathBuf,
    pub descriptor_digest: ContentDigest,
}

pub fn load_resource_catalog_snapshot() -> Result<ResourceCatalogSnapshot, ResourceCatalogError> {
    let state = ExecutionState::open().map_err(|source| ResourceCatalogError::Io {
        path: "~/.ad".into(),
        source,
    })?;
    let (document, migrated) = load_document_or_project(&state)?;
    let mut snapshot = document.snapshot(migrated);
    annotate_missing_installations(&mut snapshot)?;
    Ok(snapshot)
}

fn annotate_missing_installations(
    snapshot: &mut ResourceCatalogSnapshot,
) -> Result<(), ResourceCatalogError> {
    let missing_sources = snapshot
        .resources
        .values()
        .filter(|resource| !resource.present && resource.lifecycle == ResourceLifecycle::Managed)
        .map(|resource| (resource.id.as_str(), resource.source_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut counted = BTreeSet::new();
    for installation in super::list_resource_installations()
        .map_err(|error| ResourceCatalogError::Corrupt(error.to_string()))?
    {
        if let Some(source_id) = missing_sources.get(installation.resource_id.as_str()) {
            if counted.insert(format!("installation:{}", installation.id)) {
                *snapshot
                    .missing_installations_by_source
                    .entry((*source_id).to_owned())
                    .or_default() += 1;
            }
        }
    }
    for record in super::list_all_user_plugin_management_records()
        .map_err(|error| ResourceCatalogError::Corrupt(error.to_string()))?
    {
        if let Some(source_id) = missing_sources.get(record.resource_id.as_str()) {
            if counted.insert(format!("user-plugin:{}", record.id)) {
                *snapshot
                    .missing_installations_by_source
                    .entry((*source_id).to_owned())
                    .or_default() += 1;
            }
        }
    }
    Ok(())
}

pub fn public_resource_catalog_snapshot(
    mut snapshot: ResourceCatalogSnapshot,
) -> ResourceCatalogSnapshot {
    for source in snapshot.sources.values_mut() {
        source.binding = None;
    }
    snapshot
}

pub fn resolve_catalog_resource(
    resource_id: &str,
) -> Result<ResolvedCatalogResource, ResourceCatalogError> {
    let snapshot = load_resource_catalog_snapshot()?;
    let resource = snapshot
        .resources
        .get(resource_id)
        .ok_or_else(|| ResourceCatalogError::NotFound(resource_id.to_owned()))?;
    if !resource.present || resource.lifecycle != ResourceLifecycle::Managed {
        return Err(ResourceCatalogError::Unavailable(resource_id.to_owned()));
    }
    let source = snapshot
        .sources
        .get(&resource.source_id)
        .ok_or_else(|| ResourceCatalogError::Corrupt("resource source is missing".into()))?;
    let binding = source
        .binding
        .as_ref()
        .ok_or_else(|| ResourceCatalogError::Unavailable(resource_id.to_owned()))?;
    let relative = safe_relative(&resource.subpath)?;
    let stable_root = Path::new(&binding.stable_root);
    let physical_root = std::fs::canonicalize(&binding.physical_root).map_err(|source| {
        ResourceCatalogError::Io {
            path: binding.physical_root.clone(),
            source,
        }
    })?;
    let resolved_stable =
        std::fs::canonicalize(stable_root).map_err(|source| ResourceCatalogError::Io {
            path: binding.stable_root.clone(),
            source,
        })?;
    if resolved_stable != physical_root {
        return Err(ResourceCatalogError::Corrupt(
            "source stable root no longer resolves to its physical root".into(),
        ));
    }
    let physical_path = std::fs::canonicalize(physical_root.join(&relative)).map_err(|source| {
        ResourceCatalogError::Io {
            path: physical_root.join(&relative).display().to_string(),
            source,
        }
    })?;
    if !physical_path.starts_with(&physical_root) || !physical_path.is_dir() {
        return Err(ResourceCatalogError::Corrupt(
            "resource path escapes or is unavailable".into(),
        ));
    }
    Ok(ResolvedCatalogResource {
        resource_id: resource.id.clone(),
        source_id: resource.source_id.clone(),
        kind: resource.kind,
        install_id: resource.install_id.clone(),
        stable_path: stable_root.join(relative),
        physical_path,
        descriptor_digest: resource.descriptor_digest.clone(),
    })
}

pub(super) fn persist_resource_catalog_projection(
    state: &StateDirectory,
    skill_catalog: &SkillCatalogSnapshot,
) -> Result<Vec<u8>, ResourceCatalogError> {
    let previous = load_document(state)?;
    let revision = previous
        .as_ref()
        .map_or(1, |document| document.revision.saturating_add(1));
    let document = project_skill_catalog(skill_catalog, previous.as_ref(), revision)?;
    let bytes = document.render()?;
    state
        .write_atomic(RESOURCE_CATALOG_FILE, &bytes)
        .map_err(|source| ResourceCatalogError::Io {
            path: state
                .display_path()
                .join(RESOURCE_CATALOG_FILE)
                .display()
                .to_string(),
            source,
        })?;
    Ok(bytes)
}

fn load_document_or_project(
    state: &ExecutionState,
) -> Result<(ResourceCatalogDocument, bool), ResourceCatalogError> {
    if let Some(document) = load_document(state.state())? {
        return Ok((document, false));
    }
    let legacy = load_skill_catalog_snapshot()?;
    Ok((project_skill_catalog(&legacy, None, 0)?, true))
}

fn load_document(
    state: &StateDirectory,
) -> Result<Option<ResourceCatalogDocument>, ResourceCatalogError> {
    let bytes = match state.read(RESOURCE_CATALOG_FILE) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ResourceCatalogError::Io {
                path: state
                    .display_path()
                    .join(RESOURCE_CATALOG_FILE)
                    .display()
                    .to_string(),
                source,
            })
        }
    };
    let document: ResourceCatalogDocument = serde_json::from_slice(&bytes)
        .map_err(|error| ResourceCatalogError::Corrupt(error.to_string()))?;
    document.validate()?;
    Ok(Some(document))
}

pub(super) fn set_resource_lifecycle(
    resource_id: &str,
    lifecycle: ResourceLifecycle,
) -> Result<ResourceCatalogSnapshot, ResourceCatalogError> {
    let state = ExecutionState::open().map_err(|source| ResourceCatalogError::Io {
        path: "~/.ad".into(),
        source,
    })?;
    let operation_id = format!("resource-lifecycle:{}", uuid::Uuid::new_v4());
    let _locks = TargetLockSet::acquire_for_ad_states(
        &[
            resource_catalog_lock_target(&state),
            resource_lifecycle_lock_target(&state, resource_id),
        ],
        &operation_id,
        &state,
    )
    .map_err(|source| ResourceCatalogError::Io {
        path: "resource catalog lifecycle lock".into(),
        source,
    })?;
    write_resource_lifecycle(&state, resource_id, lifecycle)
}

pub(super) fn set_resource_lifecycle_under_lease(
    state: &ExecutionState,
    resource_id: &str,
    lifecycle: ResourceLifecycle,
) -> Result<ResourceCatalogSnapshot, ResourceCatalogError> {
    let operation_id = format!("resource-lifecycle-commit:{}", uuid::Uuid::new_v4());
    let _lock = TargetLockSet::acquire_for_ad_states(
        &[resource_catalog_lock_target(state)],
        &operation_id,
        state,
    )
    .map_err(|source| ResourceCatalogError::Io {
        path: "resource catalog writer lock".into(),
        source,
    })?;
    write_resource_lifecycle(state, resource_id, lifecycle)
}

fn write_resource_lifecycle(
    state: &ExecutionState,
    resource_id: &str,
    lifecycle: ResourceLifecycle,
) -> Result<ResourceCatalogSnapshot, ResourceCatalogError> {
    let (mut document, _) = load_document_or_project(state)?;
    let next_revision = document.revision.saturating_add(1);
    let resource = document
        .resources
        .get_mut(resource_id)
        .ok_or_else(|| ResourceCatalogError::NotFound(resource_id.to_owned()))?;
    if !resource.present {
        return Err(ResourceCatalogError::Unavailable(resource_id.to_owned()));
    }
    resource.lifecycle = lifecycle;
    resource.suppressed_at_revision =
        (lifecycle == ResourceLifecycle::Suppressed).then_some(next_revision);
    document.revision = next_revision;
    let bytes = document.render()?;
    state
        .state()
        .write_atomic(RESOURCE_CATALOG_FILE, &bytes)
        .map_err(|source| ResourceCatalogError::Io {
            path: state
                .state()
                .display_path()
                .join(RESOURCE_CATALOG_FILE)
                .display()
                .to_string(),
            source,
        })?;
    Ok(document.snapshot(false))
}

pub(super) fn resource_catalog_lock_target(state: &ExecutionState) -> PathBuf {
    state.state().display_path().join(RESOURCE_CATALOG_FILE)
}

pub(super) fn resource_lifecycle_lock_target(state: &ExecutionState, resource_id: &str) -> PathBuf {
    state.resource_installations().display_path().join(format!(
        "{}.lifecycle",
        opaque_contract_id("resource-lifecycle", &[resource_id]).replace(':', "_")
    ))
}

pub(super) fn resource_lifecycle_id(resource: &ResourceRef) -> Option<String> {
    if !matches!(resource.kind, ResourceKind::Skills | ResourceKind::Plugins) {
        return None;
    }
    let (source_id, install_id) = resource.logical_id.rsplit_once('/')?;
    source_id
        .starts_with("skill-source:")
        .then(|| catalog_resource_id(source_id, resource.kind, install_id))
}

fn project_skill_catalog(
    skill_catalog: &SkillCatalogSnapshot,
    previous: Option<&ResourceCatalogDocument>,
    revision: u64,
) -> Result<ResourceCatalogDocument, ResourceCatalogError> {
    let mut document = ResourceCatalogDocument {
        schema_version: RESOURCE_CATALOG_SCHEMA_VERSION,
        revision,
        sources: BTreeMap::new(),
        resources: BTreeMap::new(),
    };
    for entry in &skill_catalog.entries {
        let (binding, items) = projected_source(entry)?;
        document.sources.insert(
            entry.source_id.clone(),
            CatalogSource {
                id: entry.source_id.clone(),
                display_name: entry.display_name.clone(),
                source_type: entry.source_type,
                location: entry.location.clone(),
                branch: entry.branch.clone(),
                subdirectory: entry.subdirectory.clone(),
                binding: Some(binding),
                health: CatalogSourceHealth::Healthy,
            },
        );
        for item in items {
            let id = catalog_resource_id(&entry.source_id, item.kind, &item.install_id);
            let previous_resource = previous.and_then(|catalog| catalog.resources.get(&id));
            let compatible_agents = current_compatible_agents(&item);
            document.resources.insert(
                id.clone(),
                CatalogResource {
                    id,
                    source_id: entry.source_id.clone(),
                    kind: item.kind,
                    install_id: item.install_id,
                    display_name: item.display_name,
                    description: item.description,
                    version: None,
                    subpath: item.subpath,
                    descriptor_digest: item.descriptor_digest,
                    compatible_agents,
                    present: true,
                    lifecycle: previous_resource
                        .map_or(ResourceLifecycle::Managed, |resource| resource.lifecycle),
                    suppressed_at_revision: previous_resource
                        .and_then(|resource| resource.suppressed_at_revision),
                    last_diagnostic_code: None,
                },
            );
        }
        if let Some(previous) = previous {
            for old in previous
                .resources
                .values()
                .filter(|resource| resource.source_id == entry.source_id)
            {
                document
                    .resources
                    .entry(old.id.clone())
                    .or_insert_with(|| CatalogResource {
                        present: false,
                        last_diagnostic_code: Some("resource_missing_from_source".into()),
                        ..old.clone()
                    });
            }
        }
    }
    document.validate()?;
    Ok(document)
}

fn projected_source(
    entry: &SkillCatalogEntry,
) -> Result<(CatalogSourceBinding, Vec<SourceResourceItem>), ResourceCatalogError> {
    if let Some(binding) = &entry.current_binding {
        let items = if binding.resources.is_empty() {
            binding
                .skills
                .iter()
                .map(|skill| SourceResourceItem {
                    kind: ResourceKind::Skills,
                    install_id: skill.logical_id.clone(),
                    display_name: skill.logical_id.clone(),
                    description: None,
                    subpath: skill.subpath.clone(),
                    descriptor_digest: skill.instruction_digest.clone(),
                    supported_agents: BTreeSet::from(["claude-code".into(), "codex".into()]),
                })
                .collect()
        } else {
            binding.resources.clone()
        };
        return Ok((
            CatalogSourceBinding {
                binding_id: binding.binding_id.clone(),
                source_revision: binding.source_revision.clone(),
                stable_root: binding.stable_root.clone(),
                physical_root: binding.physical_root.clone(),
                tree_digest: binding.tree_digest.clone(),
                manifest_digest: binding.manifest_digest.clone(),
            },
            items,
        ));
    }
    let artifact = entry
        .current_artifact
        .as_ref()
        .ok_or_else(|| ResourceCatalogError::Corrupt("legacy source has no artifact".into()))?;
    let root = verify_skill_artifact(artifact)
        .map_err(|error| ResourceCatalogError::Corrupt(error.to_string()))?;
    Ok((
        CatalogSourceBinding {
            binding_id: artifact.artifact_id.clone(),
            source_revision: artifact.source_revision.clone(),
            stable_root: root.to_string_lossy().into_owned(),
            physical_root: root.to_string_lossy().into_owned(),
            tree_digest: artifact.tree_digest.clone(),
            manifest_digest: artifact.manifest_digest.clone(),
        },
        artifact
            .skills
            .iter()
            .map(|skill| SourceResourceItem {
                kind: ResourceKind::Skills,
                install_id: skill.logical_id.clone(),
                display_name: skill.logical_id.clone(),
                description: None,
                subpath: skill.subpath.clone(),
                descriptor_digest: skill.instruction_digest.clone(),
                supported_agents: BTreeSet::from(["claude-code".into(), "codex".into()]),
            })
            .collect(),
    ))
}

fn current_compatible_agents(item: &SourceResourceItem) -> BTreeSet<String> {
    match item.kind {
        ResourceKind::Skills => BTreeSet::from(["claude-code".into(), "codex".into()]),
        ResourceKind::Plugins => item.supported_agents.clone(),
        _ => BTreeSet::new(),
    }
}

impl ResourceCatalogDocument {
    fn snapshot(&self, migrated_from_skill_catalog: bool) -> ResourceCatalogSnapshot {
        let sources = self
            .sources
            .iter()
            .map(|(id, source)| {
                let safe_location = super::skill_catalog::format_safe_source_location(
                    source.source_type,
                    &source.location,
                )
                .unwrap_or_else(|| "Unavailable".into());
                let mut source = source.clone();
                source.location = safe_location;
                (id.clone(), source)
            })
            .collect();
        let mut resources = self.resources.clone();
        for resource in resources.values_mut() {
            resource.version = None;
            if resource.present && resource.lifecycle == ResourceLifecycle::Managed {
                resource.version = self
                    .sources
                    .get(&resource.source_id)
                    .and_then(|source| source.binding.as_ref())
                    .and_then(|binding| {
                        super::resource_version::read_resource_version(
                            Path::new(&binding.physical_root),
                            &resource.subpath,
                            resource.kind,
                        )
                    });
            }
            resource.compatible_agents = match resource.kind {
                ResourceKind::Skills => BTreeSet::from(["claude-code".into(), "codex".into()]),
                ResourceKind::Plugins => self
                    .sources
                    .get(&resource.source_id)
                    .and_then(|source| source.binding.as_ref())
                    .map(|binding| {
                        let root = Path::new(&binding.physical_root).join(&resource.subpath);
                        let mut agents = BTreeSet::new();
                        if root.join(".claude-plugin/plugin.json").is_file() {
                            agents.insert("claude-code".into());
                        }
                        if super::codex_plugins::read_codex_catalog_plugin_metadata(
                            &root,
                            &resource.install_id,
                        )
                        .is_ok()
                        {
                            agents.insert("codex".into());
                        }
                        agents
                    })
                    .unwrap_or_default(),
                _ => BTreeSet::new(),
            };
        }
        ResourceCatalogSnapshot {
            schema_version: self.schema_version,
            revision: self.revision,
            sources,
            resources,
            missing_installations_by_source: BTreeMap::new(),
            migrated_from_skill_catalog,
        }
    }

    fn render(&self) -> Result<Vec<u8>, ResourceCatalogError> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| ResourceCatalogError::Corrupt(error.to_string()))
    }

    fn validate(&self) -> Result<(), ResourceCatalogError> {
        if self.schema_version != RESOURCE_CATALOG_SCHEMA_VERSION {
            return Err(ResourceCatalogError::Corrupt(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }
        let mut identities = BTreeSet::new();
        for (id, source) in &self.sources {
            if id != &source.id {
                return Err(ResourceCatalogError::Corrupt(
                    "source map key does not match source identity".into(),
                ));
            }
        }
        for (id, resource) in &self.resources {
            if id != &resource.id || !self.sources.contains_key(&resource.source_id) {
                return Err(ResourceCatalogError::Corrupt(
                    "resource identity or source reference is invalid".into(),
                ));
            }
            safe_relative(&resource.subpath)?;
            if !identities.insert((
                resource.source_id.clone(),
                resource.kind,
                resource.install_id.to_ascii_lowercase(),
            )) {
                return Err(ResourceCatalogError::Corrupt(
                    "source contains duplicate resource install identity".into(),
                ));
            }
            if (resource.lifecycle == ResourceLifecycle::Suppressed)
                != resource.suppressed_at_revision.is_some()
            {
                return Err(ResourceCatalogError::Corrupt(
                    "suppressed lifecycle and revision disagree".into(),
                ));
            }
        }
        Ok(())
    }
}

pub fn catalog_resource_id(source_id: &str, kind: ResourceKind, install_id: &str) -> String {
    opaque_contract_id(
        "catalog-resource",
        &[source_id, kind.contract_name(), install_id],
    )
}

fn safe_relative(value: &str) -> Result<PathBuf, ResourceCatalogError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ResourceCatalogError::Corrupt(
            "resource subpath is not a safe relative path".into(),
        ));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use chrono::Utc;

    use super::*;
    use crate::agents::{SkillActivationImpact, SkillSourceBinding};

    fn digest(value: &str) -> ContentDigest {
        ContentDigest::sha256(value.as_bytes())
    }

    struct HomeGuard(Option<OsString>);

    impl HomeGuard {
        fn set(root: &Path) -> Self {
            let previous = std::env::var_os("AD_HOME");
            std::env::set_var("AD_HOME", root);
            Self(previous)
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("AD_HOME", value),
                None => std::env::remove_var("AD_HOME"),
            }
        }
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn missing_installation_counts_fail_closed_on_a_corrupt_ledger() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let state = ExecutionState::open().unwrap();
        state
            .resource_installations()
            .write_atomic("corrupt.json", b"{")
            .unwrap();
        let mut snapshot = ResourceCatalogSnapshot {
            schema_version: RESOURCE_CATALOG_SCHEMA_VERSION,
            revision: 1,
            sources: BTreeMap::new(),
            resources: BTreeMap::new(),
            missing_installations_by_source: BTreeMap::new(),
            migrated_from_skill_catalog: false,
        };

        assert!(annotate_missing_installations(&mut snapshot).is_err());
        state
            .resource_installations()
            .remove("corrupt.json")
            .unwrap();
        state
            .user_plugin_management()
            .write_atomic("corrupt.json", b"{")
            .unwrap();
        assert!(annotate_missing_installations(&mut snapshot).is_err());
    }

    #[test]
    fn projection_keeps_same_named_resources_from_different_sources_distinct() {
        let entries = ["one", "two"]
            .into_iter()
            .map(|id| SkillCatalogEntry {
                source_id: format!("skill-source:00000000-0000-0000-0000-00000000000{id}"),
                display_name: format!("Source {id}"),
                source_type: SkillSourceType::Local,
                location: format!("/tmp/{id}"),
                branch: None,
                subdirectory: None,
                auto_update: false,
                current_artifact: None,
                current_binding: Some(SkillSourceBinding {
                    schema_version: 3,
                    binding_id: format!("binding-{id}"),
                    source_id: format!("skill-source:00000000-0000-0000-0000-00000000000{id}"),
                    source_type: SkillSourceType::Local,
                    source_revision: "local:test".into(),
                    stable_root: format!("/tmp/{id}"),
                    physical_root: format!("/tmp/{id}"),
                    tree_digest: digest(id),
                    manifest_digest: digest(id),
                    skills: Vec::new(),
                    resources: vec![SourceResourceItem {
                        kind: ResourceKind::Skills,
                        install_id: "review".into(),
                        display_name: "Review".into(),
                        description: None,
                        subpath: "review".into(),
                        descriptor_digest: digest("review"),
                        supported_agents: BTreeSet::from(["claude-code".into(), "codex".into()]),
                    }],
                    activation_impact: SkillActivationImpact {
                        instructions: Vec::new(),
                        hooks: Vec::new(),
                        mcp: Vec::new(),
                        commands: Vec::new(),
                        scripts: Vec::new(),
                        binaries: Vec::new(),
                        executable_paths: Vec::new(),
                        digest: digest("impact"),
                    },
                }),
                added_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .collect();
        let snapshot = SkillCatalogSnapshot {
            schema_version: 2,
            revision: digest("catalog"),
            entries,
        };

        let projected = project_skill_catalog(&snapshot, None, 0).unwrap();

        assert_eq!(projected.resources.len(), 2);
        assert_ne!(
            projected.resources.keys().next(),
            projected.resources.keys().nth(1)
        );
    }

    #[test]
    fn snapshot_refreshes_versions_without_persisting_them_or_changing_resource_identity() {
        let temp = tempfile::tempdir().unwrap();
        let source_id = "skill-source:00000000-0000-0000-0000-000000000001";
        let resource_id = catalog_resource_id(source_id, ResourceKind::Skills, "review");
        let root = temp.path().to_string_lossy();
        // A pre-feature catalog has no version field and remains readable.
        let mut document: ResourceCatalogDocument = serde_json::from_value(serde_json::json!({
            "schemaVersion": RESOURCE_CATALOG_SCHEMA_VERSION,
            "revision": 1,
            "sources": {source_id: {
                "id": source_id, "displayName": "Tools", "sourceType": "local",
                "location": root, "health": "healthy",
                "binding": {
                    "bindingId": "binding", "sourceRevision": "local:revision",
                    "stableRoot": root, "physicalRoot": root,
                    "treeDigest": digest("tree"), "manifestDigest": digest("manifest")
                }
            }},
            "resources": {&resource_id: {
                "id": resource_id, "sourceId": source_id, "kind": "skills",
                "installId": "review", "displayName": "Review", "subpath": "",
                "descriptorDigest": digest("review"), "present": true, "lifecycle": "managed"
            }}
        }))
        .unwrap();
        let original = document.render().unwrap();
        for (declaration, expected) in [
            ("metadata: {version: '1.0.0'}", Some("1.0.0")),
            ("metadata: {version: '2.0.0'}", Some("2.0.0")),
            ("name: review", None),
        ] {
            std::fs::write(
                temp.path().join("SKILL.md"),
                format!("---\n{declaration}\n---\n"),
            )
            .unwrap();
            let snapshot = public_resource_catalog_snapshot(document.snapshot(false));
            assert_eq!(
                snapshot.resources[&resource_id].version.as_deref(),
                expected
            );
            assert_eq!(snapshot.resources[&resource_id].id, resource_id);
            assert!(snapshot.sources[source_id].binding.is_none());
            let json = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(
                json["resources"][&resource_id]
                    .get("version")
                    .and_then(serde_json::Value::as_str),
                expected
            );
            assert_eq!(document.render().unwrap(), original);
        }
        std::fs::write(temp.path().join("SKILL.md"), "---\nversion: '3.0.0'\n---\n").unwrap();
        document.resources.get_mut(&resource_id).unwrap().present = false;
        assert_eq!(
            document.snapshot(false).resources[&resource_id].version,
            None
        );
        document.resources.get_mut(&resource_id).unwrap().present = true;
        document.resources.get_mut(&resource_id).unwrap().lifecycle = ResourceLifecycle::Suppressed;
        document
            .resources
            .get_mut(&resource_id)
            .unwrap()
            .suppressed_at_revision = Some(1);
        assert_eq!(
            document.snapshot(false).resources[&resource_id].version,
            None
        );
    }

    #[test]
    fn public_snapshot_omits_internal_source_bindings() {
        let source_id = "skill-source:00000000-0000-0000-0000-000000000001";
        let mut sources = BTreeMap::new();
        sources.insert(
            source_id.into(),
            CatalogSource {
                id: source_id.into(),
                display_name: "Source".into(),
                source_type: SkillSourceType::Local,
                location: "/tmp/source".into(),
                branch: None,
                subdirectory: None,
                binding: Some(CatalogSourceBinding {
                    binding_id: "binding".into(),
                    source_revision: "local:test".into(),
                    stable_root: "/tmp/stable".into(),
                    physical_root: "/tmp/physical".into(),
                    tree_digest: digest("tree"),
                    manifest_digest: digest("manifest"),
                }),
                health: CatalogSourceHealth::Healthy,
            },
        );
        let snapshot = ResourceCatalogSnapshot {
            schema_version: RESOURCE_CATALOG_SCHEMA_VERSION,
            revision: 1,
            sources,
            resources: BTreeMap::new(),
            missing_installations_by_source: BTreeMap::new(),
            migrated_from_skill_catalog: false,
        };

        let public = public_resource_catalog_snapshot(snapshot);

        assert!(public.sources[source_id].binding.is_none());
    }
}

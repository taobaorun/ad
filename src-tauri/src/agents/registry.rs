use std::collections::BTreeSet;

use super::{
    launch_descriptor, plugins_descriptor, process_descriptor, settings_descriptor,
    skills_descriptor, AgentDefinition, AgentInstallation, AgentMetadata, Capability,
    CapabilityDescriptor, CapabilityKind, InstallationCandidate, LaunchPort, ManagedResourceTarget,
    PluginsPort, ProcessPort, ProfileSchema, ResourceKind, ResourceRef, SettingsPort, SkillsPort,
};

/// Built-in adapter boundary. User-defined adapters are intentionally not
/// supported; registration is compiled into the application.
pub trait AgentAdapter: Send + Sync {
    fn definition(&self) -> &AgentDefinition;

    fn discover(&self) -> Vec<InstallationCandidate>;

    fn settings(&self) -> Option<&dyn SettingsPort> {
        None
    }

    fn skills(&self) -> Option<&dyn SkillsPort> {
        None
    }

    fn plugins(&self) -> Option<&dyn PluginsPort> {
        None
    }

    fn processes(&self) -> Option<&dyn ProcessPort> {
        None
    }

    fn launcher(&self) -> Option<&dyn LaunchPort> {
        None
    }

    fn profile_schema(&self) -> Option<&dyn ProfileSchema> {
        None
    }
}

/// Registry for built-in Agent adapters.
#[derive(Default)]
pub struct AdapterRegistry {
    adapters: Vec<Box<dyn AgentAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Box<dyn AgentAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn metadata(&self) -> Vec<AgentMetadata> {
        self.adapters
            .iter()
            .map(|adapter| {
                let definition = adapter.definition();
                let capabilities = descriptors(adapter.as_ref())
                    .into_iter()
                    .map(|descriptor| legacy_capability(descriptor.kind))
                    .collect::<BTreeSet<_>>();
                AgentMetadata {
                    id: definition.id.clone(),
                    display_name: definition.display_name.clone(),
                    capabilities,
                }
            })
            .collect()
    }

    pub fn capability_descriptors(&self, agent_id: &str) -> Option<Vec<CapabilityDescriptor>> {
        self.adapter(agent_id).map(descriptors)
    }

    pub fn discover(&self) -> Vec<AgentInstallation> {
        let mut seen = BTreeSet::new();
        let mut installations = Vec::new();
        for candidate in self.adapters.iter().flat_map(|adapter| adapter.discover()) {
            if seen.insert(candidate.canonical_key().clone()) {
                installations.push(candidate.into_installation());
            }
        }
        installations
    }

    pub fn adapter(&self, agent_id: &str) -> Option<&dyn AgentAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.definition().id.as_str() == agent_id)
            .map(|adapter| adapter.as_ref())
    }

    pub fn adapter_for_context(
        &self,
        context: &super::AgentContext,
    ) -> Result<&dyn AgentAdapter, super::AgentError> {
        let installation = self
            .discover()
            .into_iter()
            .find(|installation| installation.id == context.installation_id);
        let agent_id = match installation {
            Some(installation) => installation.agent_id,
            None => {
                let derived_runtime = context
                    .project_path
                    .as_deref()
                    .map(std::path::Path::new)
                    .map(|project_path| {
                        super::project_runtime_descriptor_for_context(
                            &context.installation_id,
                            project_path,
                        )
                    })
                    .transpose()
                    .map_err(|error| context_registry_error(context, error.to_string()))?
                    .flatten();
                if derived_runtime.is_some() {
                    "codex".into()
                } else {
                    return Err(context_registry_error(
                        context,
                        "Unknown Agent installation",
                    ));
                }
            }
        };
        self.adapter(agent_id.as_str())
            .ok_or_else(|| context_registry_error(context, "Unknown Agent adapter"))
    }

    pub fn resolve_resource(
        &self,
        context: &super::AgentContext,
        resource: &ResourceRef,
    ) -> Result<ManagedResourceTarget, super::AgentError> {
        let adapter = self
            .adapter_for_context(context)
            .map_err(|error| with_registry_resource(error, resource))?;
        if resource.installation_id != context.installation_id {
            return Err(registry_error(
                context,
                resource,
                "Resource does not belong to the active Agent installation",
            ));
        }
        match resource.kind {
            ResourceKind::Settings => adapter
                .settings()
                .ok_or_else(|| registry_error(context, resource, "Settings are unsupported"))?
                .resolve(context, resource),
            ResourceKind::Skills => adapter
                .skills()
                .ok_or_else(|| registry_error(context, resource, "Skills are unsupported"))?
                .resolve(context, resource),
            ResourceKind::Plugins => adapter
                .plugins()
                .ok_or_else(|| registry_error(context, resource, "Plugins are unsupported"))?
                .resolve(context, resource),
            _ => Err(registry_error(
                context,
                resource,
                "Resource kind is not managed by this Agent adapter",
            )),
        }
    }
}

fn context_registry_error(
    context: &super::AgentContext,
    message: impl Into<String>,
) -> super::AgentError {
    super::AgentError {
        code: super::AgentErrorCode::Unsupported,
        message: message.into(),
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: None,
        retryable: false,
        details: None,
    }
}

fn with_registry_resource(
    mut error: super::AgentError,
    resource: &ResourceRef,
) -> super::AgentError {
    error.resource = Some(resource.clone());
    error
}

fn registry_error(
    context: &super::AgentContext,
    resource: &ResourceRef,
    message: impl Into<String>,
) -> super::AgentError {
    super::AgentError {
        code: super::AgentErrorCode::Unsupported,
        message: message.into(),
        agent_id: None,
        installation_id: Some(context.installation_id.clone()),
        resource: Some(resource.clone()),
        retryable: false,
        details: None,
    }
}

fn descriptors(adapter: &dyn AgentAdapter) -> Vec<CapabilityDescriptor> {
    let mut result = Vec::new();
    if let Some(port) = adapter.settings() {
        result.push(settings_descriptor(port));
    }
    if let Some(port) = adapter.skills() {
        result.push(skills_descriptor(port));
    }
    if let Some(port) = adapter.plugins() {
        result.push(plugins_descriptor(port));
    }
    if let Some(port) = adapter.processes() {
        result.push(process_descriptor(port));
    }
    if let Some(port) = adapter.launcher() {
        result.push(launch_descriptor(port));
    }
    result
}

fn legacy_capability(kind: CapabilityKind) -> Capability {
    match kind {
        CapabilityKind::Settings => Capability::Settings,
        CapabilityKind::Skills => Capability::Skills,
        CapabilityKind::Plugins => Capability::Plugins,
        CapabilityKind::ProcessDetection => Capability::ProcessDetection,
        CapabilityKind::TerminalLaunch => Capability::TerminalLaunch,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{AdapterRegistry, AgentAdapter};
    use crate::agents::{
        AgentContext, AgentDefinition, AgentError, CapabilityAvailability, CapabilityOperation,
        DiscoveryEvidence, InstallationCandidate, ManagedResourceTarget, MutationPlan,
        ResourcePort, ResourceRef, ResourceScope, ResourceSnapshot, SettingsEdit, SettingsPort,
    };

    struct FakeSettingsPort;

    impl ResourcePort for FakeSettingsPort {
        fn resolve(
            &self,
            _context: &AgentContext,
            _resource: &ResourceRef,
        ) -> Result<ManagedResourceTarget, AgentError> {
            unreachable!("the descriptor test does not resolve resources")
        }
    }

    impl SettingsPort for FakeSettingsPort {
        fn scopes(&self) -> BTreeSet<ResourceScope> {
            BTreeSet::from([ResourceScope::User])
        }

        fn operations(&self) -> BTreeSet<CapabilityOperation> {
            BTreeSet::from([CapabilityOperation::Inspect])
        }

        fn availability(&self) -> CapabilityAvailability {
            CapabilityAvailability::Available
        }

        fn inspect(&self, _context: &AgentContext) -> Result<Vec<ResourceSnapshot>, AgentError> {
            Ok(Vec::new())
        }

        fn plan_edit(
            &self,
            _context: &AgentContext,
            _edit: SettingsEdit,
        ) -> Result<MutationPlan, AgentError> {
            unreachable!("the descriptor test does not execute mutations")
        }
    }

    struct FakeAdapter {
        definition: AgentDefinition,
        candidates: Vec<InstallationCandidate>,
        settings: Option<FakeSettingsPort>,
    }

    impl AgentAdapter for FakeAdapter {
        fn definition(&self) -> &AgentDefinition {
            &self.definition
        }

        fn discover(&self) -> Vec<InstallationCandidate> {
            self.candidates.clone()
        }

        fn settings(&self) -> Option<&dyn SettingsPort> {
            self.settings.as_ref().map(|port| port as &dyn SettingsPort)
        }
    }

    fn fake_adapter(id: &str, settings: bool) -> FakeAdapter {
        FakeAdapter {
            definition: AgentDefinition {
                id: id.into(),
                display_name: id.into(),
                adapter_version: 1,
            },
            candidates: Vec::new(),
            settings: settings.then_some(FakeSettingsPort),
        }
    }

    #[test]
    fn registry_exposes_metadata_and_deduplicated_installations() {
        let temp = tempfile::tempdir().unwrap();
        let config_home = temp.path().join("codex");
        std::fs::create_dir_all(&config_home).unwrap();
        let direct = InstallationCandidate::from_existing_home(
            "codex",
            &config_home,
            DiscoveryEvidence::DefaultHome,
        )
        .unwrap();
        let trailing = InstallationCandidate::from_existing_home(
            "codex",
            format!("{}/", config_home.display()),
            DiscoveryEvidence::Environment,
        )
        .unwrap();
        let mut registry = AdapterRegistry::new();
        let mut adapter = fake_adapter("codex", false);
        adapter.candidates = vec![direct, trailing];
        registry.register(Box::new(adapter));

        assert_eq!(registry.metadata().len(), 1);
        assert_eq!(registry.discover().len(), 1);
        assert!(serde_json::to_value(registry.discover()).unwrap()[0]
            .get("evidence")
            .is_none());
    }

    #[test]
    fn descriptors_are_derived_from_returned_ports() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(fake_adapter("without-port", false)));
        registry.register(Box::new(fake_adapter("with-port", true)));

        assert!(registry
            .capability_descriptors("without-port")
            .unwrap()
            .is_empty());
        let descriptors = registry.capability_descriptors("with-port").unwrap();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].kind, crate::agents::CapabilityKind::Settings);
        assert_eq!(
            descriptors[0].operations,
            BTreeSet::from([CapabilityOperation::Inspect])
        );

        let metadata = registry.metadata();
        assert!(metadata[0].capabilities.is_empty());
        assert_eq!(
            metadata[1].capabilities,
            BTreeSet::from([crate::agents::Capability::Settings])
        );
    }
}

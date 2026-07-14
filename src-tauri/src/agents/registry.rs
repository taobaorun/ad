use std::collections::BTreeSet;

use super::{
    deduplicate_installations, launch_descriptor, plugins_descriptor, process_descriptor,
    settings_descriptor, skills_descriptor, AgentDefinition, AgentInstallation, AgentMetadata,
    Capability, CapabilityDescriptor, CapabilityKind, LaunchPort, PluginsPort, ProcessPort,
    SettingsPort, SkillsPort,
};

/// Built-in adapter boundary. User-defined adapters are intentionally not
/// supported; registration is compiled into the application.
pub trait AgentAdapter: Send + Sync {
    fn definition(&self) -> &AgentDefinition;

    fn discover(&self) -> Vec<AgentInstallation>;

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
        let installations = self
            .adapters
            .iter()
            .flat_map(|adapter| adapter.discover())
            .collect::<Vec<_>>();
        deduplicate_installations(installations)
    }

    pub fn adapter(&self, agent_id: &str) -> Option<&dyn AgentAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.definition().id.as_str() == agent_id)
            .map(|adapter| adapter.as_ref())
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
        AgentContext, AgentDefinition, AgentError, AgentInstallation, CapabilityAvailability,
        CapabilityOperation, MutationPlan, ResourceScope, ResourceSnapshot, SettingsEdit,
        SettingsPort,
    };

    struct FakeSettingsPort;

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
        installations: Vec<AgentInstallation>,
        settings: Option<FakeSettingsPort>,
    }

    impl AgentAdapter for FakeAdapter {
        fn definition(&self) -> &AgentDefinition {
            &self.definition
        }

        fn discover(&self) -> Vec<AgentInstallation> {
            self.installations.clone()
        }

        fn settings(&self) -> Option<&dyn SettingsPort> {
            self.settings.as_ref().map(|port| port as &dyn SettingsPort)
        }
    }

    fn fake_adapter(id: &str, root: &str, settings: bool) -> FakeAdapter {
        FakeAdapter {
            definition: AgentDefinition {
                id: id.into(),
                display_name: id.into(),
                adapter_version: 1,
            },
            installations: vec![AgentInstallation::new(id, root)],
            settings: settings.then_some(FakeSettingsPort),
        }
    }

    #[test]
    fn registry_exposes_metadata_and_deduplicated_installations() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(fake_adapter("codex", "/tmp/codex", false)));
        registry.register(Box::new(fake_adapter("codex", "/tmp/codex/", false)));

        assert_eq!(registry.metadata().len(), 2);
        assert_eq!(registry.discover().len(), 1);
    }

    #[test]
    fn descriptors_are_derived_from_returned_ports() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(fake_adapter("without-port", "/tmp/none", false)));
        registry.register(Box::new(fake_adapter("with-port", "/tmp/settings", true)));

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

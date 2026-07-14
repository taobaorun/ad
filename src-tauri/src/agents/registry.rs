use super::{deduplicate_installations, AgentInstallation, AgentMetadata};

/// Built-in adapter boundary. User-defined adapters are intentionally not
/// supported; registration is compiled into the application.
pub trait AgentAdapter: Send + Sync {
    fn metadata(&self) -> &AgentMetadata;

    fn discover(&self) -> Vec<AgentInstallation>;
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
            .map(|adapter| adapter.metadata().clone())
            .collect()
    }

    pub fn discover(&self) -> Vec<AgentInstallation> {
        let installations = self
            .adapters
            .iter()
            .flat_map(|adapter| adapter.discover())
            .collect::<Vec<_>>();
        deduplicate_installations(installations)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{AdapterRegistry, AgentAdapter};
    use crate::agents::{AgentInstallation, AgentMetadata, Capability};

    struct FakeAdapter {
        metadata: AgentMetadata,
        installations: Vec<AgentInstallation>,
    }

    impl AgentAdapter for FakeAdapter {
        fn metadata(&self) -> &AgentMetadata {
            &self.metadata
        }

        fn discover(&self) -> Vec<AgentInstallation> {
            self.installations.clone()
        }
    }

    fn fake_adapter(id: &str, root: &str) -> FakeAdapter {
        FakeAdapter {
            metadata: AgentMetadata {
                id: id.into(),
                display_name: id.into(),
                capabilities: BTreeSet::from([Capability::Settings]),
            },
            installations: vec![AgentInstallation::new(id, root)],
        }
    }

    #[test]
    fn registry_exposes_metadata_and_deduplicated_installations() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(fake_adapter("codex", "/tmp/codex")));
        registry.register(Box::new(fake_adapter("codex", "/tmp/codex/")));

        assert_eq!(registry.metadata().len(), 2);
        assert_eq!(registry.discover().len(), 1);
    }
}

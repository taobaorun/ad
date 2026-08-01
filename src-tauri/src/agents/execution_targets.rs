use super::{ManagedResourceTarget, ResourceRef};

/// AD-owned state targets are closed over this allowlist instead of accepting paths over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // U10 consumes these sealed variants when execution owns target resolution.
pub(crate) enum AdStateRef {
    SourceCatalog,
    OwnershipRecords,
    OperationHistory,
    ProjectRuntimeManifest,
}

/// Physical targets stay sealed inside the backend after resolution by an allowlisted port.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // U10 consumes these sealed variants when execution owns target resolution.
pub(crate) enum SealedMutationTarget {
    AgentResource {
        resource: ResourceRef,
        target: ManagedResourceTarget,
    },
    AdState {
        reference: AdStateRef,
        target: ManagedResourceTarget,
    },
}

impl SealedMutationTarget {
    #[allow(dead_code)]
    pub(crate) fn managed_target(&self) -> &ManagedResourceTarget {
        match self {
            Self::AgentResource { target, .. } | Self::AdState { target, .. } => target,
        }
    }
}

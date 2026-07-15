use std::collections::{BTreeMap, BTreeSet};

use super::codex_ports::{agent_error, resolve_codex_home, validate_project_path};
use super::{
    detect_processes, AgentContext, AgentError, AgentErrorCode, CapabilityAvailability,
    CapabilityOperation, LaunchPort, LaunchRecipe, ProcessMatchSpec, ProcessObservation,
    ProcessPort, ResourceScope,
};

#[derive(Debug, Default)]
pub(crate) struct CodexProcessPort;

#[derive(Debug, Default)]
pub(crate) struct CodexLaunchPort;

impl ProcessPort for CodexProcessPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([CapabilityOperation::Detect])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn match_spec(&self) -> ProcessMatchSpec {
        ProcessMatchSpec::new(["codex", "codex-cli"])
    }

    fn detect(&self, context: &AgentContext) -> Result<Vec<ProcessObservation>, AgentError> {
        resolve_codex_home(context)?;
        Ok(detect_processes(context, &self.match_spec()))
    }
}

impl LaunchPort for CodexLaunchPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([CapabilityOperation::Launch])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn recipe(&self, context: &AgentContext) -> Result<LaunchRecipe, AgentError> {
        resolve_codex_home(context)?;
        let project_path = context.project_path.as_deref().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Codex launch requires a project context",
            )
        })?;
        let project = validate_project_path(context, project_path)?;
        Ok(LaunchRecipe {
            program: "codex".into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: project.to_string_lossy().into_owned(),
        })
    }
}

use std::collections::{BTreeMap, BTreeSet};

use crate::commands::activate::detect_claude_processes_inner;

use super::super::{
    AgentContext, AgentError, AgentErrorCode, CapabilityAvailability, CapabilityOperation,
    LaunchPort, LaunchRecipe, ProcessObservation, ProcessPort, ResourceScope,
};
use super::common::{agent_error, resolve_claude_home, validate_project_path};

#[derive(Debug, Default)]
pub(crate) struct ClaudeProcessPort;

#[derive(Debug, Default)]
pub(crate) struct ClaudeLaunchPort;

impl ProcessPort for ClaudeProcessPort {
    fn scopes(&self) -> BTreeSet<ResourceScope> {
        BTreeSet::from([ResourceScope::User, ResourceScope::Project])
    }

    fn operations(&self) -> BTreeSet<CapabilityOperation> {
        BTreeSet::from([CapabilityOperation::Detect])
    }

    fn availability(&self) -> CapabilityAvailability {
        CapabilityAvailability::Available
    }

    fn detect(&self, context: &AgentContext) -> Result<Vec<ProcessObservation>, AgentError> {
        resolve_claude_home(context)?;
        Ok(detect_claude_processes_inner()
            .into_iter()
            .map(|process| ProcessObservation {
                pid: process.pid,
                installation_id: context.installation_id.clone(),
                executable: process.cmd,
                cwd: None,
            })
            .collect())
    }
}

impl LaunchPort for ClaudeLaunchPort {
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
        resolve_claude_home(context)?;
        let project_path = context.project_path.as_deref().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Claude launch requires a project context",
            )
        })?;
        let project = validate_project_path(context, project_path)?;
        Ok(LaunchRecipe {
            program: "claude".into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: project.to_string_lossy().into_owned(),
        })
    }
}

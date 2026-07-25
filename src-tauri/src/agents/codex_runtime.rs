use std::collections::{BTreeMap, BTreeSet};

use crate::fs::paths::codex_dir;

use super::codex_ports::{agent_error, resolve_codex_home, validate_project_path};
use super::{
    detect_processes, runtime_for_installation, AgentContext, AgentError, AgentErrorCode,
    CapabilityAvailability, CapabilityOperation, LaunchPort, LaunchRecipe,
    ProcessEnvironmentFilter, ProcessMatchSpec, ProcessObservation, ProcessPort, ResourceScope,
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
        let codex_home = resolve_codex_home(context)?;
        let default_home = codex_dir()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok());
        let environment = ProcessEnvironmentFilter::path(
            "CODEX_HOME",
            &codex_home,
            default_home.as_deref() == Some(codex_home.as_path()),
        );
        Ok(detect_processes(
            context,
            &self.match_spec(),
            Some(&environment),
        ))
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
        let codex_home = resolve_codex_home(context)?;
        let project_path = context.project_path.as_deref().ok_or_else(|| {
            agent_error(
                AgentErrorCode::InvalidPlan,
                context,
                None,
                "Codex launch requires a project context",
            )
        })?;
        let project = validate_project_path(context, project_path)?;
        let mut env = BTreeMap::new();
        let default_home = codex_dir()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok());
        if default_home.as_deref() != Some(codex_home.as_path()) {
            env.insert(
                "CODEX_HOME".to_string(),
                codex_home.to_string_lossy().into_owned(),
            );
        }
        let args = runtime_for_installation(&context.installation_id)
            .and_then(|runtime| runtime.profile_id)
            .map(|profile_id| vec!["--profile".to_string(), profile_id])
            .unwrap_or_default();
        Ok(LaunchRecipe {
            program: "codex".into(),
            args,
            env,
            cwd: project.to_string_lossy().into_owned(),
        })
    }
}

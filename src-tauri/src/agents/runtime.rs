use std::ffi::OsString;
use std::path::{Path, PathBuf};

use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

use super::{AgentContext, ProcessMatchSpec, ProcessObservation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEnvironmentFilter {
    variable: String,
    expected_path: PathBuf,
    allow_missing: bool,
}

impl ProcessEnvironmentFilter {
    pub fn path(
        variable: impl Into<String>,
        expected_path: impl Into<PathBuf>,
        allow_missing: bool,
    ) -> Self {
        Self {
            variable: variable.into(),
            expected_path: expected_path.into(),
            allow_missing,
        }
    }
}

pub fn detect_processes(
    context: &AgentContext,
    spec: &ProcessMatchSpec,
    environment: Option<&ProcessEnvironmentFilter>,
) -> Vec<ProcessObservation> {
    let me = std::process::id();
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(
            ProcessRefreshKind::nothing()
                .with_cwd(UpdateKind::OnlyIfNotSet)
                .with_environ(UpdateKind::OnlyIfNotSet),
        ),
    );
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter(|(pid, process)| {
            pid.as_u32() != me
                && spec.matches(&process.name().to_string_lossy())
                && project_cwd_matches(
                    context.project_path.as_deref().map(Path::new),
                    process.cwd(),
                )
                && environment_matches(environment, process.environ())
        })
        .map(|(pid, process)| ProcessObservation {
            pid: pid.as_u32(),
            installation_id: context.installation_id.clone(),
            executable: process.name().to_string_lossy().into_owned(),
            cwd: process
                .cwd()
                .map(|path| path.to_string_lossy().into_owned()),
        })
        .collect()
}

fn project_cwd_matches(project_path: Option<&Path>, process_cwd: Option<&Path>) -> bool {
    match (project_path, process_cwd) {
        (None, _) => true,
        (Some(project_path), Some(process_cwd)) => process_cwd.starts_with(project_path),
        (Some(_), None) => false,
    }
}

fn environment_matches(
    filter: Option<&ProcessEnvironmentFilter>,
    environment: &[OsString],
) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let actual = environment.iter().find_map(|entry| {
        entry
            .to_string_lossy()
            .split_once('=')
            .and_then(|(name, value)| (name == filter.variable).then(|| PathBuf::from(value)))
    });
    let Some(actual) = actual else {
        return filter.allow_missing;
    };
    equivalent_path(&actual, &filter.expected_path)
}

fn equivalent_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_uses_exact_case_insensitive_basenames() {
        let spec = ProcessMatchSpec::new(["codex", "codex-cli"]);
        assert!(spec.matches("Codex"));
        assert!(spec.matches("codex.exe"));
        assert!(!spec.matches("codex-helper"));
    }

    #[test]
    fn context_match_requires_the_requested_project_and_installation_environment() {
        let project = Path::new("/Users/test/project");
        assert!(project_cwd_matches(
            Some(project),
            Some(Path::new("/Users/test/project/subdir"))
        ));
        assert!(!project_cwd_matches(
            Some(project),
            Some(Path::new("/Users/test/other"))
        ));
        assert!(!project_cwd_matches(Some(project), None));

        let filter = ProcessEnvironmentFilter::path(
            "CODEX_HOME",
            "/Users/test/.ad/codex-homes/project",
            false,
        );
        assert!(environment_matches(
            Some(&filter),
            &[OsString::from(
                "CODEX_HOME=/Users/test/.ad/codex-homes/project"
            )]
        ));
        assert!(!environment_matches(
            Some(&filter),
            &[OsString::from("CODEX_HOME=/Users/test/.codex")]
        ));
        assert!(!environment_matches(Some(&filter), &[]));
    }
}

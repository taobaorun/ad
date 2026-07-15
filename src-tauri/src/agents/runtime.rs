use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

use super::{AgentContext, ProcessMatchSpec, ProcessObservation};

pub fn detect_processes(
    context: &AgentContext,
    spec: &ProcessMatchSpec,
) -> Vec<ProcessObservation> {
    let me = std::process::id();
    let mut system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_cwd(UpdateKind::OnlyIfNotSet)),
    );
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter(|(pid, process)| {
            pid.as_u32() != me && spec.matches(&process.name().to_string_lossy())
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
}

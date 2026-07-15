use std::collections::BTreeSet;

use ad_lib::agents::{
    builtin_registry, ArtifactDisposition, ClaudeToCodexRoute, ConversionRoute, WritePolicy,
};
use serial_test::serial;

#[test]
#[serial(home_env)]
fn route_reports_artifact_dispositions_and_builds_a_target_only_plan() {
    let home = tempfile::tempdir().unwrap();
    let claude_home = home.path().join(".claude");
    let codex_home = home.path().join(".codex");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    create_enabled_claude_skill(home.path(), &claude_home);
    let source_bytes = include_bytes!("fixtures/conversion/claude-settings.json");
    let target_bytes = include_bytes!("fixtures/conversion/codex-config.toml");
    std::fs::write(claude_home.join("settings.json"), source_bytes).unwrap();
    std::fs::write(codex_home.join("config.toml"), target_bytes).unwrap();

    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", home.path());
    std::env::remove_var("CODEX_HOME");

    let registry = builtin_registry();
    let installations = registry.discover();
    let source = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "claude-code")
        .unwrap()
        .context(None);
    let target = installations
        .iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap()
        .context(None);

    let route = ClaudeToCodexRoute;
    let result = route.preview(&source, &target).unwrap();

    restore_env(previous_home, previous_codex_home);

    let dispositions = result
        .artifacts
        .iter()
        .map(|artifact| artifact.disposition)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        dispositions,
        BTreeSet::from([
            ArtifactDisposition::Exact,
            ArtifactDisposition::Mapped,
            ArtifactDisposition::RequiresInput,
            ArtifactDisposition::Unsupported,
            ArtifactDisposition::Conflict,
            ArtifactDisposition::Unchanged,
        ])
    );
    let skill = result
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "skill:review")
        .unwrap();
    assert_eq!(skill.disposition, ArtifactDisposition::RequiresInput);
    assert_eq!(skill.kind, ad_lib::agents::ResourceKind::Skills);

    assert!(result.plan.read_set.iter().any(|precondition| {
        precondition.resource.installation_id == source.installation_id
            && precondition.write_policy == WritePolicy::ReadOnly
    }));
    assert!(result
        .plan
        .mutations
        .iter()
        .all(|mutation| mutation.resource.installation_id == target.installation_id));
    assert!(result
        .plan
        .mutations
        .iter()
        .all(|mutation| mutation.resource.installation_id != source.installation_id));

    let content = result.plan.mutations[0]
        .content
        .as_ref()
        .unwrap()
        .as_str()
        .unwrap();
    let value = content.parse::<toml::Value>().unwrap();
    assert_eq!(value["model"].as_str(), Some("gpt-5.4"));
    assert_eq!(value["model_verbosity"].as_str(), Some("high"));
    assert_eq!(value["model_reasoning_effort"].as_str(), Some("high"));
    assert_eq!(value["sandbox_mode"].as_str(), Some("workspace-write"));
    assert_eq!(value["custom_target"].as_bool(), Some(true));
    assert_eq!(
        std::fs::read(claude_home.join("settings.json")).unwrap(),
        source_bytes
    );
    assert_eq!(
        std::fs::read(codex_home.join("config.toml")).unwrap(),
        target_bytes
    );
}

fn create_enabled_claude_skill(home: &std::path::Path, claude_home: &std::path::Path) {
    let source_root = home.join(".ad/skill-library/local");
    let skill = source_root.join("review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: review\ndescription: Review changes\n---\n",
    )
    .unwrap();
    let state = home.join(".ad/state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("skill_sources.json"),
        serde_json::to_vec(&serde_json::json!([{
            "id": "local",
            "sourceType": "local",
            "url": source_root,
            "autoUpdate": false,
            "addedAt": "2026-07-15T00:00:00Z"
        }]))
        .unwrap(),
    )
    .unwrap();
    let skills = claude_home.join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(skill, skills.join("review")).unwrap();
}

fn restore_env(previous_home: Option<String>, previous_codex_home: Option<String>) {
    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }
}

trait InstallationContext {
    fn context(&self, project_path: Option<String>) -> ad_lib::agents::AgentContext;
}

impl InstallationContext for ad_lib::agents::AgentInstallation {
    fn context(&self, project_path: Option<String>) -> ad_lib::agents::AgentContext {
        ad_lib::agents::AgentContext {
            installation_id: self.id.clone(),
            project_path,
        }
    }
}

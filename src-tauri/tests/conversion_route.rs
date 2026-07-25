use std::collections::BTreeSet;

use ad_lib::agents::{
    builtin_registry, ArtifactDisposition, ClaudeToCodexOptions, ClaudeToCodexRoute,
    ConversionProgressEvent, ConversionProgressPhase, ConversionResolutionKind,
    ConversionRiskLevel, ResourceKind, WritePolicy,
};
use serial_test::serial;
use std::cell::RefCell;

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
    let progress = RefCell::new(Vec::new());
    let result = route
        .preview_with_options_and_progress(
            &source,
            &target,
            &ClaudeToCodexOptions {
                target_model: Some("gpt-5.4".into()),
                permission_preset: None,
                confirmed_skill_ids: BTreeSet::from(["review".into()]),
                profile_id: None,
                inherit_base_config: true,
            },
            &|event| progress.borrow_mut().push(event),
        )
        .unwrap();
    let unknown_skill = route
        .preview_with_options(
            &source,
            &target,
            &ClaudeToCodexOptions {
                confirmed_skill_ids: BTreeSet::from(["missing-skill".into()]),
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap_err();
    let isolated_profile = route
        .preview_with_options(
            &source,
            &target,
            &ClaudeToCodexOptions {
                profile_id: Some("project-api".into()),
                inherit_base_config: false,
                ..ClaudeToCodexOptions::default()
            },
        )
        .unwrap_err();

    restore_env(previous_home, previous_codex_home);
    assert!(unknown_skill.message.contains("missing-skill"));
    assert!(isolated_profile
        .message
        .contains("requires Base config inheritance"));
    assert_eq!(
        progress
            .into_inner()
            .into_iter()
            .map(|event: ConversionProgressEvent| event.phase)
            .collect::<Vec<_>>(),
        vec![
            ConversionProgressPhase::ReadingConfiguration,
            ConversionProgressPhase::InspectingSkills,
            ConversionProgressPhase::InspectingPlugins,
            ConversionProgressPhase::FinalizingPlan,
        ]
    );

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
    assert_eq!(skill.disposition, ArtifactDisposition::Mapped);
    assert_eq!(skill.kind, ResourceKind::Skills);
    assert_eq!(skill.risk, ConversionRiskLevel::Confirmation);
    assert!(skill.resolution.is_none());
    assert!(skill
        .source
        .location
        .path
        .ends_with("/.claude/skills/review"));
    assert!(skill
        .target
        .as_ref()
        .unwrap()
        .location
        .path
        .ends_with("/.agents/skills/review"));

    let unresolved_permission = result
        .artifacts
        .iter()
        .find(|artifact| artifact.id.ends_with(":permissions"))
        .unwrap();
    assert_eq!(
        unresolved_permission.resolution.as_ref().unwrap().kind,
        ConversionResolutionKind::SelectPermissionPreset
    );
    assert_eq!(result.summary.total, result.artifacts.len());
    assert_eq!(
        result.summary.requires_input,
        result
            .artifacts
            .iter()
            .filter(|artifact| artifact.disposition == ArtifactDisposition::RequiresInput)
            .count()
    );

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
    assert!(result.plan.mutations.iter().any(|mutation| {
        mutation.resource.kind == ResourceKind::Skills
            && mutation.resource.logical_id == "review"
            && mutation.media_type == "application/vnd.ad.symlink"
    }));

    let content = result
        .plan
        .mutations
        .iter()
        .find(|mutation| mutation.resource.kind == ResourceKind::Settings)
        .unwrap()
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

#[test]
fn claude_to_codex_options_default_to_base_config_inheritance() {
    let omitted = serde_json::from_value::<ClaudeToCodexOptions>(serde_json::json!({})).unwrap();
    let explicit_off = serde_json::from_value::<ClaudeToCodexOptions>(
        serde_json::json!({"inheritBaseConfig": false}),
    )
    .unwrap();

    assert!(omitted.inherit_base_config);
    assert!(ClaudeToCodexOptions::default().inherit_base_config);
    assert!(!explicit_off.inherit_base_config);
}

impl InstallationContext for ad_lib::agents::AgentInstallation {
    fn context(&self, project_path: Option<String>) -> ad_lib::agents::AgentContext {
        ad_lib::agents::AgentContext {
            installation_id: self.id.clone(),
            project_path,
        }
    }
}

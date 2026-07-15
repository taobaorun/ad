mod common;
mod plugins;
mod runtime;
mod settings;
mod skills;

pub(crate) use plugins::ClaudePluginsPort;
pub(crate) use runtime::{ClaudeLaunchPort, ClaudeProcessPort};
pub(crate) use settings::ClaudeSettingsPort;
pub(crate) use skills::ClaudeSkillsPort;

#[cfg(test)]
mod tests {
    use super::super::*;

    fn setup() -> (tempfile::TempDir, AgentContext, Vec<u8>) {
        let temp = tempfile::tempdir().unwrap();
        let claude_home = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_home).unwrap();
        let content = br#"{"model":"claude-opus-4-7"}"#.to_vec();
        std::fs::write(claude_home.join("settings.json"), &content).unwrap();
        std::env::set_var("AD_HOME", temp.path());
        std::env::remove_var("CODEX_HOME");

        let installation = builtin_registry()
            .discover()
            .into_iter()
            .find(|installation| installation.agent_id.as_str() == "claude-code")
            .unwrap();
        let context = AgentContext {
            installation_id: installation.id,
            project_path: None,
        };
        (temp, context, content)
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_inspects_claude_json_with_sha256_digest() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();

        let first = port.inspect(&context).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].resource.kind, ResourceKind::Settings);
        assert_eq!(first[0].resource.scope, ResourceScope::User);
        assert_eq!(first[0].media_type, "application/json");
        assert!(first[0].digest.as_str().starts_with("sha256:"));

        std::fs::write(
            _temp.path().join(".claude/settings.json"),
            br#"{"model":"claude-sonnet-4-5"}"#,
        )
        .unwrap();
        let second = port.inspect(&context).unwrap();
        assert_ne!(first[0].digest, second[0].digest);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_plans_an_edit_without_writing_source() {
        let (temp, context, original) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();
        let snapshot = port.inspect(&context).unwrap().remove(0);

        let plan = port
            .plan_edit(
                &context,
                SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/json".into(),
                    content: serde_json::json!({"model": "claude-sonnet-4-5"}),
                },
            )
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Replace);
        assert!(plan.mutations[0].expected_digest.is_some());
        assert_eq!(
            std::fs::read(temp.path().join(".claude/settings.json")).unwrap(),
            original
        );
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn settings_port_rejects_non_object_json() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().settings().unwrap();
        let snapshot = port.inspect(&context).unwrap().remove(0);

        let error = port
            .plan_edit(
                &context,
                SettingsEdit {
                    resource: snapshot.resource,
                    media_type: "application/json".into(),
                    content: serde_json::json!("not-an-object"),
                },
            )
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::InvalidPlan);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn process_port_returns_standard_observations() {
        let (_temp, context, _) = setup();
        let registry = builtin_registry();
        let port = registry
            .adapter("claude-code")
            .unwrap()
            .processes()
            .unwrap();

        let observations = port.detect(&context).unwrap();

        assert!(observations
            .iter()
            .all(|process| process.installation_id == context.installation_id));
        assert!(observations
            .iter()
            .all(|process| !process.executable.is_empty()));
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn launch_port_builds_a_claude_recipe_for_the_project_context() {
        let (temp, mut context, _) = setup();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().launcher().unwrap();

        let recipe = port.recipe(&context).unwrap();

        assert_eq!(recipe.program, "claude");
        assert_eq!(recipe.cwd, context.project_path.unwrap());
        assert!(recipe.args.is_empty());
        assert!(recipe.env.is_empty());
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn skills_port_lists_and_plans_project_enable_without_writing() {
        let (temp, mut context, _) = setup();
        let skill = temp.path().join(".ad/skill-library/source/demo");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().skills().unwrap();

        let snapshots = port.list(&context).unwrap();
        assert!(!temp.path().join(".ad/state/skill_sources.json").exists());
        let demo = snapshots
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "source/demo")
            .unwrap();
        let plan = port
            .plan_set_enabled(&context, &demo.resource, true)
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Create);
        assert_eq!(plan.mutations[0].media_type, "application/vnd.ad.symlink");
        assert!(!project.join(".claude/skills/demo").exists());
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn skills_port_refuses_to_replace_an_unmanaged_symlink() {
        let (temp, mut context, _) = setup();
        let skill = temp.path().join(".ad/skill-library/source/demo");
        let project = temp.path().join("project");
        let unmanaged = temp.path().join("unmanaged");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(&unmanaged).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        std::os::unix::fs::symlink(&unmanaged, project.join(".claude/skills/demo")).unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().skills().unwrap();
        let demo = port
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "source/demo")
            .unwrap();

        let error = port
            .plan_set_enabled(&context, &demo.resource, true)
            .unwrap_err();

        assert_eq!(error.code, AgentErrorCode::PermissionDenied);
    }

    #[test]
    #[serial_test::serial(home_env)]
    fn plugins_port_lists_and_plans_project_override_without_writing() {
        let (temp, mut context, _) = setup();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            temp.path().join(".claude/settings.json"),
            br#"{"enabledPlugins":{"demo":true}}"#,
        )
        .unwrap();
        context.project_path = Some(
            std::fs::canonicalize(&project)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        let registry = builtin_registry();
        let port = registry.adapter("claude-code").unwrap().plugins().unwrap();

        let plugin = port
            .list(&context)
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.resource.logical_id == "demo")
            .unwrap();
        let plan = port
            .plan_set_enabled(&context, &plugin.resource, false)
            .unwrap();

        assert_eq!(plan.mutations.len(), 1);
        assert_eq!(plan.mutations[0].kind, MutationKind::Create);
        assert_eq!(
            plan.mutations[0].content.as_ref().unwrap()["enabledPlugins"]["demo"],
            false
        );
        assert!(!project.join(".claude/settings.local.json").exists());
    }
}

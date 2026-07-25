use std::path::Path;

use ad_lib::agents::{
    classify_claude_plugin, inspect_claude_plugin, prepare_project_plugin_install,
    ClaudePluginRoute,
};

fn write_json(path: &Path, value: serde_json::Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn fixture(
    temp: &tempfile::TempDir,
    plugin: &str,
    package_content: impl FnOnce(&Path),
) -> (std::path::PathBuf, std::path::PathBuf, String) {
    let claude_home = temp.path().join(".claude");
    let project = temp.path().join("project");
    let package = claude_home
        .join("plugins/cache/team")
        .join(plugin)
        .join("1.2.3");
    let marketplace = claude_home.join("plugins/marketplaces/team");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&package).unwrap();
    std::fs::create_dir_all(&marketplace).unwrap();
    package_content(&package);
    let plugin_id = format!("{plugin}@team");
    write_json(
        &claude_home.join("plugins/installed_plugins.json"),
        serde_json::json!({
            "version": 2,
            "plugins": {
                plugin_id.clone(): [{
                    "scope": "local",
                    "projectPath": std::fs::canonicalize(&project).unwrap(),
                    "installPath": package,
                    "version": "1.2.3",
                    "gitCommitSha": "abc123"
                }]
            }
        }),
    );
    write_json(
        &claude_home.join("plugins/known_marketplaces.json"),
        serde_json::json!({
            "team": {
                "source": {"source": "github", "repo": "acme/plugins"},
                "installLocation": claude_home.join("plugins/marketplaces/team")
            }
        }),
    );
    (claude_home, project, plugin_id)
}

#[test]
fn disabled_plugin_is_unchanged_without_resolving_a_package() {
    let classification = classify_claude_plugin(None, false);

    assert_eq!(classification.route, ClaudePluginRoute::SourceDisabled);
    assert!(classification.residual_reasons.is_empty());
}

#[test]
fn native_codex_package_uses_package_copy_route() {
    let temp = tempfile::tempdir().unwrap();
    let (claude_home, project, plugin_id) = fixture(&temp, "native", |package| {
        write_json(
            &package.join(".codex-plugin/plugin.json"),
            serde_json::json!({"name": "native", "version": "1.2.3"}),
        );
    });

    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();
    let classification = classify_claude_plugin(Some(&descriptor), true);

    assert_eq!(classification.route, ClaudePluginRoute::PackageCopy);
    assert_eq!(descriptor.marketplace.source_type, "git");
    assert_eq!(
        descriptor.marketplace.source,
        "https://github.com/acme/plugins.git"
    );
    assert_eq!(descriptor.marketplace.last_revision, None);
    assert_eq!(
        descriptor.source_digest,
        ad_lib::agents::directory_tree_digest(&descriptor.source_root).unwrap()
    );
}

#[test]
fn portable_claude_package_uses_transform_and_partial_routes() {
    let temp = tempfile::tempdir().unwrap();
    let (claude_home, project, plugin_id) = fixture(&temp, "portable", |package| {
        std::fs::create_dir_all(package.join("skills/review")).unwrap();
        std::fs::write(
            package.join("skills/review/SKILL.md"),
            "---\nname: review\n---\n",
        )
        .unwrap();
        write_json(
            &package.join(".claude-plugin/plugin.json"),
            serde_json::json!({"name": "portable", "version": "1.2.3"}),
        );
    });
    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();
    assert_eq!(
        classify_claude_plugin(Some(&descriptor), true).route,
        ClaudePluginRoute::PackageTransform
    );

    write_json(
        &descriptor.source_root.join(".lsp.json"),
        serde_json::json!({"demo": {"command": "demo-lsp"}}),
    );
    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();
    let partial = classify_claude_plugin(Some(&descriptor), true);
    assert_eq!(partial.route, ClaudePluginRoute::Partial);
    assert!(partial
        .residual_reasons
        .iter()
        .any(|reason| reason.contains("LSP")));
}

#[test]
fn lsp_only_plugin_is_unsupported_with_a_specific_reason() {
    let temp = tempfile::tempdir().unwrap();
    let (claude_home, project, plugin_id) = fixture(&temp, "jdtls-lsp", |package| {
        write_json(
            &package.join(".claude-plugin/plugin.json"),
            serde_json::json!({
                "name": "jdtls-lsp",
                "version": "1.2.3",
                "lspServers": {"jdtls": {"command": "jdtls"}}
            }),
        );
    });

    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();
    let classification = classify_claude_plugin(Some(&descriptor), true);

    assert_eq!(
        classification.route,
        ClaudePluginRoute::UnsupportedComponent
    );
    assert_eq!(
        descriptor.source_digest.as_str().split(':').next(),
        Some("sha256")
    );
    assert!(classification.residual_reasons[0].contains("LSP"));
}

#[test]
#[serial_test::serial(home_env)]
fn portable_package_is_prepared_in_ad_staging_with_a_codex_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let (claude_home, project, plugin_id) = fixture(&temp, "portable", |package| {
        std::fs::create_dir_all(package.join("commands")).unwrap();
        std::fs::write(package.join("commands/review.md"), "Review this change.").unwrap();
        write_json(
            &package.join(".claude-plugin/plugin.json"),
            serde_json::json!({"name": "portable", "version": "1.2.3"}),
        );
    });
    write_json(
        &claude_home.join("plugins/marketplaces/team/.agents/plugins/marketplace.json"),
        serde_json::json!({"name": "team"}),
    );
    let previous_home = std::env::var("AD_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();

    let prepared = prepare_project_plugin_install(&descriptor).unwrap();

    let package_stage = Path::new(prepared.source["package"]["stagePath"].as_str().unwrap());
    assert!(package_stage.starts_with(temp.path().join(".ad/staging")));
    assert!(package_stage.join(".codex-plugin/plugin.json").is_file());
    assert_eq!(
        std::fs::read_to_string(package_stage.join("skills/review/SKILL.md")).unwrap(),
        "Review this change."
    );
    assert_eq!(prepared.source["package"]["version"], "1.2.3");

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
}

#[test]
#[serial_test::serial(home_env)]
fn staging_identity_separates_identical_trees_with_different_plugin_identity() {
    let temp = tempfile::tempdir().unwrap();
    let (claude_home, project, plugin_id) = fixture(&temp, "native", |package| {
        write_json(
            &package.join(".codex-plugin/plugin.json"),
            serde_json::json!({"name": "native", "version": "1.2.3"}),
        );
    });
    write_json(
        &claude_home.join("plugins/marketplaces/team/.agents/plugins/marketplace.json"),
        serde_json::json!({"name": "team"}),
    );
    let previous_home = std::env::var("AD_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    let descriptor = inspect_claude_plugin(&claude_home, &project, &plugin_id, true).unwrap();
    let mut alternate = descriptor.clone();
    alternate.plugin_id = "native@other".into();
    alternate.marketplace.name = "other".into();

    let first = prepare_project_plugin_install(&descriptor).unwrap();
    let second = prepare_project_plugin_install(&alternate).unwrap();

    let first_package = Path::new(first.source["package"]["stagePath"].as_str().unwrap());
    let second_package = Path::new(second.source["package"]["stagePath"].as_str().unwrap());
    assert_ne!(first_package.parent(), second_package.parent());
    assert!(first_package.join(".codex-plugin/plugin.json").is_file());
    assert!(second_package.join(".codex-plugin/plugin.json").is_file());

    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
}

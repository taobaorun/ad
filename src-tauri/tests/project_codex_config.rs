use std::collections::BTreeMap;

use ad_lib::agents::{
    synthesize_project_codex_config, synthesize_project_codex_config_with_settings,
    MarketplaceOverlay, ProjectPluginOverlay,
};

#[test]
fn synthesis_preserves_unknown_base_config_and_adds_project_plugins() {
    let temp = tempfile::tempdir().unwrap();
    let base_dir = temp.path().join("base");
    std::fs::create_dir_all(&base_dir).unwrap();
    let base = concat!(
        "model = \"gpt-5.6\"\n",
        "unknown_future_key = true\n\n",
        "[mcp_servers.demo]\n",
        "command = \"demo\"\n",
        "cwd = \"tools/demo\"\n\n",
        "[plugins.\"existing@base\"]\n",
        "enabled = true\n",
        "future = 7\n",
    );
    let overlay = ProjectPluginOverlay {
        marketplaces: BTreeMap::from([(
            "claude-plugins-official".to_string(),
            MarketplaceOverlay {
                source_type: "git".to_string(),
                source: "https://github.com/anthropics/claude-plugins-official.git".to_string(),
                ref_name: Some("main".to_string()),
                last_revision: Some("abc123".to_string()),
            },
        )]),
        enabled_plugins: BTreeMap::from([(
            "typescript-lsp@claude-plugins-official".to_string(),
            true,
        )]),
    };

    let result =
        synthesize_project_codex_config(Some(base.as_bytes()), &base_dir, &overlay).unwrap();
    let parsed = result.content.parse::<toml::Value>().unwrap();

    assert_eq!(parsed["model"].as_str(), Some("gpt-5.6"));
    assert_eq!(parsed["unknown_future_key"].as_bool(), Some(true));
    assert_eq!(
        parsed["plugins"]["existing@base"]["future"].as_integer(),
        Some(7)
    );
    assert_eq!(
        parsed["plugins"]["typescript-lsp@claude-plugins-official"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        parsed["marketplaces"]["claude-plugins-official"]["last_revision"].as_str(),
        Some("abc123")
    );
    assert_eq!(parsed["cli_auth_credentials_store"].as_str(), Some("file"));
    assert_eq!(
        parsed["mcp_servers"]["demo"]["cwd"].as_str(),
        Some(base_dir.join("tools/demo").to_string_lossy().as_ref())
    );
    assert!(result.base_config_digest.is_some());
    assert_eq!(
        result.generated_config_digest,
        ad_lib::agents::ContentDigest::sha256(result.content.as_bytes())
    );
}

#[test]
fn synthesis_reuses_matching_marketplace_and_rejects_source_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let base_dir = temp.path();
    let base = concat!(
        "[marketplaces.team]\n",
        "source_type = \"git\"\n",
        "source = \"https://github.com/acme/plugins.git\"\n",
        "ref = \"main\"\n",
    );
    let matching = ProjectPluginOverlay {
        marketplaces: BTreeMap::from([(
            "team".to_string(),
            MarketplaceOverlay {
                source_type: "git".to_string(),
                source: "https://github.com/acme/plugins.git".to_string(),
                ref_name: Some("main".to_string()),
                last_revision: Some("revision-2".to_string()),
            },
        )]),
        enabled_plugins: BTreeMap::new(),
    };
    let result =
        synthesize_project_codex_config(Some(base.as_bytes()), base_dir, &matching).unwrap();
    let parsed = result.content.parse::<toml::Value>().unwrap();
    assert_eq!(
        parsed["marketplaces"]["team"]["last_revision"].as_str(),
        Some("revision-2")
    );

    let conflicting = ProjectPluginOverlay {
        marketplaces: BTreeMap::from([(
            "team".to_string(),
            MarketplaceOverlay {
                source_type: "git".to_string(),
                source: "https://github.com/other/plugins.git".to_string(),
                ref_name: Some("main".to_string()),
                last_revision: None,
            },
        )]),
        enabled_plugins: BTreeMap::new(),
    };
    let error =
        synthesize_project_codex_config(Some(base.as_bytes()), base_dir, &conflicting).unwrap_err();
    assert!(error.to_string().contains("different source"));
}

#[test]
fn synthesis_normalizes_all_supported_relative_path_fields() {
    let temp = tempfile::tempdir().unwrap();
    let base_dir = temp.path().join("base");
    std::fs::create_dir_all(&base_dir).unwrap();
    let base = concat!(
        "sqlite_home = \"state\"\n\n",
        "[agents.reviewer]\n",
        "config_file = \"agents/reviewer.toml\"\n\n",
        "[marketplaces.local]\n",
        "source_type = \"local\"\n",
        "source = \"marketplaces/local\"\n\n",
        "[[skills.config]]\n",
        "path = \"skills/demo\"\n",
        "enabled = true\n",
    );

    let result = synthesize_project_codex_config(
        Some(base.as_bytes()),
        &base_dir,
        &ProjectPluginOverlay::default(),
    )
    .unwrap();
    let parsed = result.content.parse::<toml::Value>().unwrap();

    assert_eq!(
        parsed["sqlite_home"].as_str(),
        Some(base_dir.join("state").to_string_lossy().as_ref())
    );
    assert_eq!(
        parsed["agents"]["reviewer"]["config_file"].as_str(),
        Some(
            base_dir
                .join("agents/reviewer.toml")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        parsed["marketplaces"]["local"]["source"].as_str(),
        Some(
            base_dir
                .join("marketplaces/local")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        parsed["skills"]["config"][0]["path"].as_str(),
        Some(base_dir.join("skills/demo").to_string_lossy().as_ref())
    );
}

#[test]
fn synthesis_keeps_project_settings_while_rebuilding_managed_plugins() {
    let temp = tempfile::tempdir().unwrap();
    let project_settings = BTreeMap::from([
        ("model".into(), toml::Value::String("project-api".into())),
        (
            "approval_policy".into(),
            toml::Value::String("never".into()),
        ),
    ]);
    let overlay = ProjectPluginOverlay {
        marketplaces: BTreeMap::new(),
        enabled_plugins: BTreeMap::from([("review@team".into(), false)]),
    };

    let result = synthesize_project_codex_config_with_settings(
        Some(b"model = \"base\"\napproval_policy = \"on-request\"\n"),
        temp.path(),
        &overlay,
        &project_settings,
    )
    .unwrap();
    let parsed = result.content.parse::<toml::Value>().unwrap();

    assert_eq!(parsed["model"].as_str(), Some("project-api"));
    assert_eq!(parsed["approval_policy"].as_str(), Some("never"));
    assert_eq!(
        parsed["plugins"]["review@team"]["enabled"].as_bool(),
        Some(false)
    );
}

#[test]
fn synthesis_rejects_project_settings_that_own_managed_fields() {
    for key in ["cli_auth_credentials_store", "marketplaces", "plugins"] {
        let error = synthesize_project_codex_config_with_settings(
            None,
            std::path::Path::new("/tmp"),
            &ProjectPluginOverlay::default(),
            &BTreeMap::from([(key.into(), toml::Value::Boolean(true))]),
        )
        .unwrap_err();

        assert!(error.to_string().contains(key));
    }
}

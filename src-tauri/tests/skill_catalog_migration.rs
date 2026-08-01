use std::path::Path;

use ad_lib::agents::{
    inspect_legacy_skill_inventory, LegacySkillHealth, LegacySkillLinkTargetKind,
};
use serial_test::serial;

fn write_legacy_source_registry(home: &Path, sources: serde_json::Value) {
    let path = home.join(".ad/state/skill_sources.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&sources).unwrap()).unwrap();
}

fn write_legacy_project_state(home: &Path, file: &str, config: &serde_json::Value) {
    let path = home.join(".ad/state/project_skills").join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(config).unwrap()).unwrap();
}

#[test]
#[serial(home_env)]
fn legacy_inventory_is_read_only_idempotent_and_blocks_ambiguous_ownership() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let checkout = home.path().join(".ad/skill-library/shared/review");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("SKILL.md"), "# Review\n").unwrap();
    let external = home.path().join("external-skill");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("SKILL.md"), "# External\n").unwrap();
    let sentinel = home.path().join("outside-sentinel");
    std::fs::write(&sentinel, "unchanged").unwrap();
    write_legacy_source_registry(
        home.path(),
        serde_json::json!([
            {
                "id": "shared",
                "sourceType": "git",
                "url": "https://example.com/shared.git",
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            },
            {
                "id": "../outside",
                "sourceType": "git",
                "url": "https://token@example.com/private.git",
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            },
            {
                "id": "missing",
                "sourceType": "local",
                "url": home.path().join("missing").to_string_lossy(),
                "autoUpdate": false,
                "addedAt": "2026-08-01T08:00:00Z"
            }
        ]),
    );
    let project = home.path().join("project");
    std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(&checkout, project.join(".claude/skills/review")).unwrap();
    std::os::unix::fs::symlink(&external, project.join(".claude/skills/external")).unwrap();
    let config = serde_json::json!({
        "projectPath": project.to_string_lossy(),
        "listedSkills": ["shared/review", "unknown/missing"],
        "mode": "allowlist"
    });
    write_legacy_project_state(home.path(), "first.json", &config);
    write_legacy_project_state(home.path(), "alias.json", &config);
    let registry_before = std::fs::read(home.path().join(".ad/state/skill_sources.json")).unwrap();
    let first = inspect_legacy_skill_inventory().unwrap();
    let second = inspect_legacy_skill_inventory().unwrap();

    assert_eq!(first, second);
    assert_eq!(
        std::fs::read(home.path().join(".ad/state/skill_sources.json")).unwrap(),
        registry_before
    );
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "unchanged");
    let unsafe_source = first
        .sources
        .iter()
        .find(|source| source.legacy_id == "../outside")
        .unwrap();
    assert_eq!(unsafe_source.health, LegacySkillHealth::Blocked);
    assert!(!unsafe_source.display_location.contains("token"));
    assert!(first.projects.iter().all(|project| {
        project.health == LegacySkillHealth::Blocked && project.missing_source_ids == ["unknown"]
    }));
    let links = &first.projects[0].links;
    assert!(
        links.iter().any(|link| {
            link.logical_id == "review"
                && link.target_kind == LegacySkillLinkTargetKind::LegacyCheckout
        }),
        "unexpected legacy links: {links:?}"
    );
    assert!(links.iter().any(|link| {
        link.logical_id == "external"
            && link.target_kind == LegacySkillLinkTargetKind::External
            && link.health == LegacySkillHealth::Blocked
    }));
}

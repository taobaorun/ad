use std::path::Path;

use chrono::Utc;
use serial_test::serial;

use super::skill_source_bindings::{
    inspect_local_skill_source_binding, publish_staged_git_skill_source_binding,
    reconcile_git_skill_source_current, skill_source_key, stage_existing_git_checkout_for_test,
    switch_git_skill_source_binding,
};
use crate::fs::paths::{skill_acquisition_staging_dir, skill_artifacts_dir};
use crate::models::{SkillSource, SkillSourceType};

fn local_source(root: &Path) -> SkillSource {
    SkillSource {
        id: "local-source".into(),
        source_type: SkillSourceType::Local,
        url: root.to_string_lossy().into_owned(),
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: Utc::now(),
    }
}

fn git_source() -> SkillSource {
    SkillSource {
        id: "git-source".into(),
        source_type: SkillSourceType::Git,
        url: "https://example.com/team/skills.git".into(),
        branch: None,
        subdirectory: None,
        auto_update: false,
        added_at: Utc::now(),
    }
}

fn staged_git_checkout(body: &str, revision: &str) -> super::StagedGitSkillSourceBinding {
    let operation = skill_acquisition_staging_dir()
        .unwrap()
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(operation.join("source/review")).unwrap();
    std::fs::write(
        operation.join("source/review/SKILL.md"),
        format!("---\nname: review\n---\n{body}"),
    )
    .unwrap();
    stage_existing_git_checkout_for_test(&git_source(), operation, revision).unwrap()
}

#[test]
#[serial(home_env)]
fn local_binding_points_at_the_original_checkout_without_publishing_an_artifact() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    std::fs::create_dir_all(source.path().join("review/scripts")).unwrap();
    std::fs::write(
        source.path().join("review/SKILL.md"),
        "---\nname: review\n---\nReview changes.",
    )
    .unwrap();
    std::fs::write(source.path().join("review/scripts/check.sh"), "#!/bin/sh\n").unwrap();

    let binding = inspect_local_skill_source_binding(&local_source(source.path())).unwrap();
    let canonical = std::fs::canonicalize(source.path()).unwrap();

    assert_eq!(binding.stable_root, canonical.to_string_lossy());
    assert_eq!(binding.physical_root, canonical.to_string_lossy());
    assert_eq!(binding.source_id, "local-source");
    assert_eq!(binding.skills[0].logical_id, "review");
    assert_eq!(binding.skills[0].subpath, "review");
    assert!(binding
        .activation_impact
        .scripts
        .contains(&"review/scripts/check.sh".into()));
    assert!(!skill_artifacts_dir().unwrap().exists());
}

#[test]
#[serial(home_env)]
fn git_binding_switches_one_stable_current_link_and_can_compensate() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let first_staged = staged_git_checkout("first", &"a".repeat(40));

    let (first, first_publication) =
        publish_staged_git_skill_source_binding(first_staged, None).unwrap();
    first_publication.commit();
    let stable_root = Path::new(&first.stable_root);
    let first_link_target = std::fs::read_link(stable_root).unwrap();
    let projects = home.path().join("projects");
    let project_a = projects.join("a/review");
    let project_b = projects.join("b/review");
    std::fs::create_dir_all(project_a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(project_b.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(stable_root.join("review"), &project_a).unwrap();
    std::os::unix::fs::symlink(stable_root.join("review"), &project_b).unwrap();
    let project_a_target = std::fs::read_link(&project_a).unwrap();
    let project_b_target = std::fs::read_link(&project_b).unwrap();

    assert!(first_link_target.starts_with("generations"));
    assert!(std::fs::read_to_string(stable_root.join("review/SKILL.md"))
        .unwrap()
        .contains("first"));

    let second_staged = staged_git_checkout("second", &"b".repeat(40));
    let (second, second_publication) =
        publish_staged_git_skill_source_binding(second_staged, Some(&first)).unwrap();

    assert_eq!(second.stable_root, first.stable_root);
    assert_ne!(std::fs::read_link(stable_root).unwrap(), first_link_target);
    assert!(std::fs::read_to_string(stable_root.join("review/SKILL.md"))
        .unwrap()
        .contains("second"));
    assert!(std::fs::read_to_string(project_a.join("SKILL.md"))
        .unwrap()
        .contains("second"));
    assert!(std::fs::read_to_string(project_b.join("SKILL.md"))
        .unwrap()
        .contains("second"));
    assert_eq!(std::fs::read_link(&project_a).unwrap(), project_a_target);
    assert_eq!(std::fs::read_link(&project_b).unwrap(), project_b_target);
    reconcile_git_skill_source_current(&second, Some(&first), true).unwrap();

    second_publication.compensate().unwrap();

    assert_eq!(std::fs::read_link(stable_root).unwrap(), first_link_target);
    assert!(std::fs::read_to_string(stable_root.join("review/SKILL.md"))
        .unwrap()
        .contains("first"));

    let third_staged = staged_git_checkout("third", &"c".repeat(40));
    let (third, third_publication) =
        publish_staged_git_skill_source_binding(third_staged, Some(&first)).unwrap();
    reconcile_git_skill_source_current(&third, Some(&first), false).unwrap();
    third_publication.commit();

    assert_eq!(std::fs::read_link(stable_root).unwrap(), first_link_target);
    assert!(std::fs::read_to_string(project_a.join("SKILL.md"))
        .unwrap()
        .contains("first"));
    assert!(std::fs::read_to_string(project_b.join("SKILL.md"))
        .unwrap()
        .contains("first"));
    assert_eq!(std::fs::read_link(&project_a).unwrap(), project_a_target);
    assert_eq!(std::fs::read_link(&project_b).unwrap(), project_b_target);
}

#[test]
#[serial(home_env)]
fn git_binding_rejects_a_symlinked_managed_source_root() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let staged = staged_git_checkout("first", &"a".repeat(40));
    let library = home.path().join(".ad/skill-library");
    std::fs::create_dir_all(&library).unwrap();
    std::os::unix::fs::symlink(outside.path(), library.join(skill_source_key("git-source")))
        .unwrap();

    let error = publish_staged_git_skill_source_binding(staged, None).unwrap_err();

    assert!(matches!(error, super::SkillArtifactError::InvalidSource(_)));
    assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[test]
#[serial(home_env)]
fn git_binding_can_switch_back_to_a_published_generation_and_compensate() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let first_staged = staged_git_checkout("first", &"a".repeat(40));
    let (first, first_publication) =
        publish_staged_git_skill_source_binding(first_staged, None).unwrap();
    first_publication.commit();
    let second_staged = staged_git_checkout("second", &"b".repeat(40));
    let (second, second_publication) =
        publish_staged_git_skill_source_binding(second_staged, Some(&first)).unwrap();
    second_publication.commit();

    let rollback = switch_git_skill_source_binding(&first, &second).unwrap();
    assert!(
        std::fs::read_to_string(Path::new(&first.stable_root).join("review/SKILL.md"))
            .unwrap()
            .contains("first")
    );

    rollback.compensate().unwrap();
    assert!(
        std::fs::read_to_string(Path::new(&second.stable_root).join("review/SKILL.md"))
            .unwrap()
            .contains("second")
    );
}

#[test]
#[serial(home_env)]
fn git_binding_rejects_a_receipt_root_not_derived_from_its_source_id() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    let staged = staged_git_checkout("first", &"a".repeat(40));
    let binding = staged.binding().clone();
    let mut tampered = binding.clone();
    let foreign_root = home.path().join("foreign-source");
    tampered.stable_root = foreign_root.join("current").to_string_lossy().into_owned();
    tampered.physical_root = foreign_root
        .join("generations/attacker")
        .to_string_lossy()
        .into_owned();

    let error = switch_git_skill_source_binding(&tampered, &tampered).unwrap_err();

    assert!(matches!(error, super::SkillArtifactError::Corrupt(_)));
}

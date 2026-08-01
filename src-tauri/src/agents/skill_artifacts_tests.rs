use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use chrono::Utc;
use serial_test::serial;

use super::skill_artifacts::{
    cleanup_unpublished_skill_staging, publish_staged_skill_artifact, stage_skill_source,
    verify_skill_artifact, SkillArtifactError,
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

fn write_skill(root: &Path, body: &str) {
    std::fs::create_dir_all(root.join("review/scripts")).unwrap();
    std::fs::write(
        root.join("review/SKILL.md"),
        format!("---\nname: review\n---\n{body}"),
    )
    .unwrap();
    std::fs::write(root.join("review/scripts/check.sh"), "#!/bin/sh\n").unwrap();
}

#[test]
#[serial(home_env)]
fn local_source_publishes_an_immutable_verified_artifact() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");

    let staged = stage_skill_source(&local_source(source.path())).unwrap();
    let reference = staged.reference().clone();
    assert!(!skill_artifacts_dir().unwrap().exists());
    let published = publish_staged_skill_artifact(staged).unwrap();
    let tree = verify_skill_artifact(&published).unwrap();

    assert_eq!(reference, published);
    assert_eq!(published.skills[0].logical_id, "review");
    assert!(tree.join("review/SKILL.md").is_file());
    assert!(published
        .activation_impact
        .scripts
        .contains(&"review/scripts/check.sh".into()));
}

#[test]
#[serial(home_env)]
fn source_refresh_creates_a_new_artifact_without_mutating_the_old_tree() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let first =
        publish_staged_skill_artifact(stage_skill_source(&local_source(source.path())).unwrap())
            .unwrap();
    let first_tree = verify_skill_artifact(&first).unwrap();
    let first_bytes = std::fs::read(first_tree.join("review/SKILL.md")).unwrap();

    write_skill(source.path(), "second");
    let second =
        publish_staged_skill_artifact(stage_skill_source(&local_source(source.path())).unwrap())
            .unwrap();

    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(
        std::fs::read(first_tree.join("review/SKILL.md")).unwrap(),
        first_bytes
    );
    assert_ne!(
        std::fs::read(
            verify_skill_artifact(&second)
                .unwrap()
                .join("review/SKILL.md")
        )
        .unwrap(),
        first_bytes
    );
}

#[test]
#[serial(home_env)]
fn existing_artifact_collision_is_revalidated_and_never_overwritten() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let first =
        publish_staged_skill_artifact(stage_skill_source(&local_source(source.path())).unwrap())
            .unwrap();
    let tree = verify_skill_artifact(&first).unwrap();
    let file = tree.join("review/SKILL.md");
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_mode(0o644);
    std::fs::set_permissions(&file, permissions).unwrap();
    std::fs::write(&file, "tampered").unwrap();

    let staged = stage_skill_source(&local_source(source.path())).unwrap();
    let error = publish_staged_skill_artifact(staged).unwrap_err();

    assert!(matches!(error, SkillArtifactError::Corrupt(_)));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "tampered");
}

#[test]
#[serial(home_env)]
fn unpublished_staging_cleanup_preserves_referenced_operations() {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::env::set_var("AD_HOME", home.path());
    write_skill(source.path(), "first");
    let active = stage_skill_source(&local_source(source.path())).unwrap();
    let active_id = active.operation_id().unwrap().to_owned();
    let root = skill_acquisition_staging_dir().unwrap();
    let first_id = uuid::Uuid::new_v4().to_string();
    let second_id = uuid::Uuid::new_v4().to_string();
    for id in [&first_id, &second_id] {
        std::fs::create_dir(root.join(id)).unwrap();
        std::fs::write(root.join(id).join(".lease"), []).unwrap();
    }

    let removed = cleanup_unpublished_skill_staging(&BTreeSet::from([first_id.clone()])).unwrap();

    assert_eq!(removed, 1);
    assert!(root.join(first_id).is_dir());
    assert!(!root.join(second_id).exists());
    assert!(root.join(active_id).is_dir());
    drop(active);
}

use std::path::{Path, PathBuf};

use super::execution_confinement::ConfinedTarget;
use super::execution_fs::TargetState;
use super::{InstallationId, ResourceKind, ResourceRef, ResourceScope};

fn project_resource(project: &Path) -> ResourceRef {
    ResourceRef {
        installation_id: InstallationId::from("claude-code:test"),
        project_path: Some(project.to_string_lossy().into_owned()),
        kind: ResourceKind::Skills,
        scope: ResourceScope::Project,
        logical_id: "demo".into(),
    }
}

#[test]
#[serial_test::serial(home_env)]
fn resolving_a_missing_target_does_not_create_parent_directories() {
    let temp = tempfile::tempdir().unwrap();
    let project = std::fs::canonicalize(temp.path()).unwrap();
    let parent = project.join(".claude/skills");
    let resource = project_resource(&project);

    let target = ConfinedTarget::resolve(&resource, &parent.join("demo")).unwrap();
    let state = target.observe().unwrap();

    assert!(matches!(state, TargetState::Missing));
    assert!(!parent.exists());
}

#[test]
#[serial_test::serial(home_env)]
fn held_parent_fd_prevents_symlink_ancestor_swap_escape() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let original_parent = project.join(".claude/skills");
    let moved_parent = project.join(".claude/skills.original");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&original_parent).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"outside").unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let resource = project_resource(&project);
    let confined =
        ConfinedTarget::resolve(&resource, &project.join(".claude/skills/demo")).unwrap();

    std::fs::rename(&original_parent, &moved_parent).unwrap();
    std::os::unix::fs::symlink(&outside, &original_parent).unwrap();
    confined
        .write_symlink_atomic(Path::new("../../source/demo"))
        .unwrap();

    assert_eq!(
        std::fs::read_link(moved_parent.join("demo")).unwrap(),
        PathBuf::from("../../source/demo")
    );
    assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"outside");
    assert!(!outside.join("demo").exists());
}

#[test]
#[serial_test::serial(home_env)]
fn held_parent_fd_prevents_directory_ancestor_swap_escape() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let original_parent = project.join(".claude/plugins");
    let moved_parent = project.join(".claude/plugins.original");
    let outside = temp.path().join("outside");
    let source = temp.path().join("source");
    std::fs::create_dir_all(original_parent.join("demo")).unwrap();
    std::fs::write(original_parent.join("demo/old.txt"), b"old").unwrap();
    std::fs::create_dir_all(outside.join("demo")).unwrap();
    std::fs::write(outside.join("demo/sentinel"), b"outside").unwrap();
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("new.txt"), b"new").unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let resource = project_resource(&project);
    let confined =
        ConfinedTarget::resolve(&resource, &project.join(".claude/plugins/demo")).unwrap();

    std::fs::rename(&original_parent, &moved_parent).unwrap();
    std::os::unix::fs::symlink(&outside, &original_parent).unwrap();
    confined.write_directory_atomic(&source).unwrap();

    assert_eq!(
        std::fs::read(moved_parent.join("demo/new.txt")).unwrap(),
        b"new"
    );
    assert_eq!(
        std::fs::read(outside.join("demo/sentinel")).unwrap(),
        b"outside"
    );
    assert!(!outside.join("demo/new.txt").exists());
}

#[test]
#[serial_test::serial(home_env)]
fn confined_directory_digest_matches_existing_contract() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let target = project.join(".claude/plugins/demo");
    std::fs::create_dir_all(target.join("nested")).unwrap();
    std::fs::write(target.join("nested/plugin.json"), b"{}\n").unwrap();
    std::os::unix::fs::symlink("nested/plugin.json", target.join("plugin-link")).unwrap();
    let project = std::fs::canonicalize(project).unwrap();
    let target = project.join(".claude/plugins/demo");
    let resource = project_resource(&project);
    let confined = ConfinedTarget::resolve(&resource, &target).unwrap();

    let TargetState::Directory(actual) = confined.observe().unwrap() else {
        panic!("expected a confined directory state");
    };
    assert_eq!(
        actual,
        super::execution_fs::directory_tree_digest(&target).unwrap()
    );
}

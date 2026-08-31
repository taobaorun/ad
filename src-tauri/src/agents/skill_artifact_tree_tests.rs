use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;

use super::skill_artifact_tree::{copy_tree_verified, inspect_tree, ArtifactLimits};

fn write_skill(root: &Path, body: &str) {
    std::fs::create_dir_all(root.join("demo/scripts")).unwrap();
    std::fs::write(root.join("demo/SKILL.md"), body).unwrap();
    std::fs::write(root.join("demo/scripts/run.sh"), "#!/bin/sh\n").unwrap();
    let mut permissions = std::fs::metadata(root.join("demo/scripts/run.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(root.join("demo/scripts/run.sh"), permissions).unwrap();
}

#[test]
fn manifest_tracks_content_and_executable_mode_but_excludes_caches() {
    let temp = tempfile::tempdir().unwrap();
    write_skill(temp.path(), "# Demo\n");
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    std::fs::write(temp.path().join(".git/config"), "secret").unwrap();

    let first = inspect_tree(temp.path(), ArtifactLimits::default()).unwrap();
    std::fs::write(temp.path().join("demo/SKILL.md"), "# Changed\n").unwrap();
    let second = inspect_tree(temp.path(), ArtifactLimits::default()).unwrap();

    assert_ne!(first.digest().unwrap(), second.digest().unwrap());
    assert!(first
        .entries
        .iter()
        .all(|entry| !entry.path.starts_with(".git")));
    assert_eq!(
        first
            .entries
            .iter()
            .find(|entry| entry.path.ends_with("run.sh"))
            .unwrap()
            .mode,
        0o755
    );
}

#[test]
fn verified_copy_preserves_relative_links_and_matches_manifest() {
    let source = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    write_skill(source.path(), "# Demo\n");
    std::os::unix::fs::symlink("SKILL.md", source.path().join("demo/README.md")).unwrap();
    let manifest = inspect_tree(source.path(), ArtifactLimits::default()).unwrap();
    let destination = stage.path().join("tree");

    copy_tree_verified(
        source.path(),
        &destination,
        &manifest,
        ArtifactLimits::default(),
    )
    .unwrap();

    assert_eq!(
        inspect_tree(&destination, ArtifactLimits::default()).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read_link(destination.join("demo/README.md")).unwrap(),
        Path::new("SKILL.md")
    );
}

#[test]
fn hostile_filesystem_entries_fail_closed() {
    let source = tempfile::tempdir().unwrap();
    write_skill(source.path(), "# Demo\n");
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), source.path().join("escape")).unwrap();
    assert!(inspect_tree(source.path(), ArtifactLimits::default()).is_err());
    std::fs::remove_file(source.path().join("escape")).unwrap();
    UnixListener::bind(source.path().join("socket")).unwrap();
    assert!(inspect_tree(source.path(), ArtifactLimits::default()).is_err());
}

#[test]
fn hardlinks_are_rejected() {
    let source = tempfile::tempdir().unwrap();
    write_skill(source.path(), "# Demo\n");
    std::fs::hard_link(
        source.path().join("demo/SKILL.md"),
        source.path().join("demo/COPY.md"),
    )
    .unwrap();
    assert!(inspect_tree(source.path(), ArtifactLimits::default()).is_err());
}

#[test]
fn aggregate_size_above_64_mib_is_accepted() {
    let source = tempfile::tempdir().unwrap();
    write_skill(source.path(), "# Demo\n");
    for index in 0..5 {
        std::fs::File::create(source.path().join(format!("asset-{index}.bin")))
            .unwrap()
            .set_len(13 * 1024 * 1024)
            .unwrap();
    }

    inspect_tree(source.path(), ArtifactLimits::default()).unwrap();
}

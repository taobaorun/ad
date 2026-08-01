use super::execution_state::ExecutionState;

#[test]
fn held_state_directories_prevent_root_swap_escape() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(".ad");
    let moved = temp.path().join(".ad.original");
    let outside = temp.path().join("outside");
    let state = ExecutionState::open_at(&root).unwrap();
    std::fs::rename(&root, &moved).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();

    state
        .journals()
        .write_atomic("operation.json", b"journal")
        .unwrap();
    state
        .backups()
        .create_directory("receipt")
        .unwrap()
        .write_atomic("manifest.json", b"manifest")
        .unwrap();
    state
        .history()
        .write_atomic("receipt.json", b"receipt")
        .unwrap();
    state.locks().open_lock("target.lock").unwrap();

    assert_eq!(
        std::fs::read(moved.join("state/operation-journals/operation.json")).unwrap(),
        b"journal"
    );
    assert_eq!(
        std::fs::read(moved.join("backups/operations/receipt/manifest.json")).unwrap(),
        b"manifest"
    );
    assert_eq!(
        std::fs::read(moved.join("history/operations/receipt.json")).unwrap(),
        b"receipt"
    );
    assert!(moved.join("state/execution-locks/target.lock").exists());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn held_journal_directory_prevents_child_swap_escape() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(".ad");
    let original = root.join("state/operation-journals.original");
    let journal_path = root.join("state/operation-journals");
    let outside = temp.path().join("outside");
    let state = ExecutionState::open_at(&root).unwrap();
    std::fs::rename(&journal_path, &original).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &journal_path).unwrap();

    state
        .journals()
        .write_atomic("operation.json", b"journal")
        .unwrap();

    assert_eq!(
        std::fs::read(original.join("operation.json")).unwrap(),
        b"journal"
    );
    assert!(!outside.join("operation.json").exists());
}

#[test]
fn state_file_names_must_be_single_components() {
    let temp = tempfile::tempdir().unwrap();
    let state = ExecutionState::open_at(&temp.path().join(".ad")).unwrap();

    let error = state
        .journals()
        .write_atomic("../escape.json", b"no")
        .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!temp.path().join("escape.json").exists());
}

#[test]
fn lock_files_reject_hard_links_to_external_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(".ad");
    let state = ExecutionState::open_at(&root).unwrap();
    let external = temp.path().join("external.txt");
    std::fs::write(&external, b"sentinel").unwrap();
    std::fs::hard_link(&external, root.join("state/execution-locks/target.lock")).unwrap();

    let error = state.locks().open_lock("target.lock").unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(std::fs::read(external).unwrap(), b"sentinel");
}

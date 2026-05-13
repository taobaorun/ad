//! Integration test that proves the atomic-write invariant under a real
//! subprocess crash, not the in-process pseudo-crash used in the unit tests.

use std::process::Command;
use tempfile::TempDir;

#[test]
fn real_subprocess_crash_preserves_original() {
    let tmp = TempDir::new().expect("temp");
    let target = tmp.path().join("settings.json");
    std::fs::write(&target, b"original").expect("write original");

    let helper = env!("CARGO_BIN_EXE_crash_helper");
    let status = Command::new(helper)
        .arg(&target)
        .arg("crashed-payload")
        .status()
        .expect("spawn helper");

    assert!(
        !status.success(),
        "helper should have aborted, exit code = {status:?}"
    );

    // Invariant 1: the canonical target file is byte-identical to before.
    assert_eq!(
        std::fs::read(&target).expect("read target"),
        b"original",
        "canonical settings.json must be untouched after subprocess crash"
    );

    // Invariant 2: the temp sibling does exist and contains the new payload —
    // proving the helper actually fsynced before aborting (i.e. the test
    // didn't no-op).
    let temps: Vec<_> = std::fs::read_dir(tmp.path())
        .expect("read tempdir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.contains(".tmp.") && s != "settings.json"
        })
        .collect();
    assert_eq!(
        temps.len(),
        1,
        "expected exactly one orphaned temp file, got: {:?}",
        temps.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );
    assert_eq!(
        std::fs::read(temps[0].path()).expect("read temp"),
        b"crashed-payload",
        "orphaned temp must contain the new payload"
    );
}

#[test]
fn real_subprocess_crash_when_target_does_not_exist() {
    let tmp = TempDir::new().expect("temp");
    let target = tmp.path().join("settings.json");
    // Note: target does NOT exist beforehand.

    let helper = env!("CARGO_BIN_EXE_crash_helper");
    let status = Command::new(helper)
        .arg(&target)
        .arg("crashed-payload")
        .status()
        .expect("spawn helper");
    assert!(!status.success());

    // The canonical target was never created (the rename never happened).
    assert!(
        !target.exists(),
        "canonical path must not exist if rename never happened"
    );

    // Temp sibling holds the payload.
    let temps: Vec<_> = std::fs::read_dir(tmp.path())
        .expect("read tempdir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert_eq!(temps.len(), 1);
}

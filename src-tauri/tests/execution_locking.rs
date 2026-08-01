use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ad_lib::agents::{execution_instance_id, TargetLockSet};

#[test]
fn lock_holder_process() {
    let Some(target) = std::env::var_os("AD_LOCK_HELPER_TARGET") else {
        return;
    };
    let ready = std::env::var_os("AD_LOCK_HELPER_READY").expect("ready path");
    let lock_root = std::env::var_os("AD_LOCK_HELPER_ROOT").expect("lock root");
    let lock_root = std::path::PathBuf::from(lock_root);
    let target = std::path::PathBuf::from(target);
    let _locks = TargetLockSet::acquire_at(
        &lock_root,
        &[target],
        execution_instance_id(),
        "helper-operation",
    )
    .expect("helper acquires target lock");
    std::fs::write(ready, "ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn target_lock_conflicts_across_processes() {
    let temp = tempfile::tempdir().unwrap();
    let target_parent = temp.path().join("project/.claude");
    std::fs::create_dir_all(&target_parent).unwrap();
    let target = target_parent.join("settings.json");
    let lock_root = temp.path().join("locks");
    let ready = temp.path().join("ready");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_holder_process", "--nocapture"])
        .env("AD_LOCK_HELPER_TARGET", &target)
        .env("AD_LOCK_HELPER_READY", &ready)
        .env("AD_LOCK_HELPER_ROOT", &lock_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.exists(), "helper did not acquire the target lock");

    let error = TargetLockSet::acquire_at(
        &lock_root,
        &[target],
        execution_instance_id(),
        "contending-operation",
    )
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn lock_metadata_contains_version_instance_and_operation() {
    let temp = tempfile::tempdir().unwrap();
    let target_parent = temp.path().join("project/.codex");
    std::fs::create_dir_all(&target_parent).unwrap();
    let lock_root = temp.path().join("locks");
    let _locks = TargetLockSet::acquire_at(
        &lock_root,
        &[target_parent.join("config.toml")],
        "instance-test",
        "operation-test",
    )
    .unwrap();
    let lock_path = std::fs::read_dir(lock_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(lock_path).unwrap()).unwrap();

    assert_eq!(metadata["schemaVersion"], 1);
    assert_eq!(metadata["instanceId"], "instance-test");
    assert_eq!(metadata["operationId"], "operation-test");
}

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use ad_lib::agents::recover_execution_state;

#[test]
#[serial_test::serial(recovery_home)]
fn real_process_crash_boundaries_recover_or_fail_closed() {
    for boundary in ["prepared_empty", "prepared", "applying", "receipt"] {
        let temp = tempfile::tempdir().unwrap();
        let status = Command::new(env!("CARGO_BIN_EXE_recovery_crash_helper"))
            .arg(temp.path())
            .arg(boundary)
            .status()
            .unwrap();
        assert!(!status.success(), "{boundary} helper must abort");
        std::env::set_var("AD_HOME", temp.path());

        let report = recover_execution_state().unwrap();
        let journal: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temp.path().join(".ad/state/operation-journals/crash.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(journal["schemaVersion"], 2);
        match boundary {
            "prepared" | "prepared_empty" => {
                assert!(report.writable());
                assert_eq!(journal["state"], "compensated");
                assert!(!temp
                    .path()
                    .join(".ad/backups/operations/crash-receipt")
                    .exists());
                assert_eq!(
                    std::fs::read(temp.path().join("target.txt")).unwrap(),
                    b"original"
                );
            }
            "applying" => {
                assert!(!report.writable());
                assert_eq!(report.repair_required, 1);
                assert_eq!(journal["state"], "repair_required");
                assert_eq!(
                    std::fs::read(temp.path().join("target.txt")).unwrap(),
                    b"changed"
                );
            }
            "receipt" => {
                assert!(report.writable());
                assert_eq!(journal["state"], "committed");
                assert_eq!(
                    std::fs::read(temp.path().join("target.txt")).unwrap(),
                    b"changed"
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
#[serial_test::serial(recovery_home)]
fn startup_recovery_lock_conflicts_across_processes() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_recovery_crash_helper"))
        .arg(temp.path())
        .arg("lock")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "locked");
    std::env::set_var("AD_HOME", temp.path());

    let error = recover_execution_state().unwrap_err();

    assert!(error.retryable);
    child.kill().unwrap();
    child.wait().unwrap();
}

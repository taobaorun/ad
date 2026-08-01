use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use rustix::fs::{flock, FlockOperation};

fn main() {
    let mut args = std::env::args().skip(1);
    let home = args.next().expect("arg1: home");
    let boundary = args.next().expect("arg2: boundary");
    let home = Path::new(&home);
    let ad = home.join(".ad");
    if boundary == "lock" {
        let locks = ad.join("state/execution-locks");
        std::fs::create_dir_all(&locks).unwrap();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(locks.join("recovery.lock"))
            .unwrap();
        flock(&file, FlockOperation::LockExclusive).unwrap();
        println!("locked");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let journals = ad.join("state/operation-journals");
    let backup_root = ad.join("backups/operations");
    let backups = backup_root.join("crash-receipt");
    let history = ad.join("history/operations");
    std::fs::create_dir_all(&journals).unwrap();
    std::fs::create_dir_all(&backup_root).unwrap();
    std::fs::create_dir_all(&history).unwrap();
    if boundary != "prepared_empty" {
        std::fs::create_dir_all(&backups).unwrap();
        durable_write(&backups.join("marker"), b"backup");
    }
    durable_write(&home.join("target.txt"), b"original");

    let state = match boundary.as_str() {
        "prepared" | "prepared_empty" => "prepared",
        "applying" | "receipt" => "applying",
        _ => panic!("unknown boundary"),
    };
    let journal = serde_json::json!({
        "schemaVersion": 1,
        "instanceId": "crash-helper",
        "operationId": format!("crash-{boundary}"),
        "planId": "crash-plan",
        "plannedReceiptId": "crash-receipt",
        "state": state,
        "targets": [],
    });
    durable_write(
        &journals.join("crash.json"),
        &serde_json::to_vec(&journal).unwrap(),
    );

    if !matches!(boundary.as_str(), "prepared" | "prepared_empty") {
        durable_write(&home.join("target.txt"), b"changed");
    }
    if boundary == "receipt" {
        let receipt = serde_json::json!({
            "id": "crash-receipt",
            "planId": "crash-plan",
            "status": "complete",
            "appliedResources": [],
            "backupPaths": [],
            "postApplyStates": [],
        });
        durable_write(
            &history.join("crash-receipt.json"),
            &serde_json::to_vec(&receipt).unwrap(),
        );
    }
    std::process::abort();
}

fn durable_write(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn private_singleton_daemon_sleeps_without_probe_extending_idle() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("sanc-daemon-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(root.join("bin")).unwrap();
    let mock = root.join("bin/ssh");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ssh"),
        &mock,
    )
    .unwrap();
    std::fs::set_permissions(mock, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut db = sessanchor::worker::store(&root).unwrap();
    db.configure(
        "dev",
        &sessanchor::ssh::Target {
            host: "example.test".into(),
            user: "tester".into(),
            port: 22,
            identity: root.join("key"),
        },
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sanc"))
        .arg("--state-dir")
        .arg(&root)
        .args([
            "daemon",
            "run",
            "--idle-seconds",
            "1",
            "--probe-seconds",
            "1",
        ])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("SANC_TEST_CALLS", root.join("calls"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !root.join("daemon.sock").exists() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("daemon failed to start");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = (|| -> Result<(), String> {
        let again = Command::new(env!("CARGO_BIN_EXE_sanc"))
            .arg("--state-dir")
            .arg(&root)
            .args(["daemon", "run"])
            .output()
            .map_err(|e| e.to_string())?;
        if again.status.success() {
            return Err("second daemon allowed".into());
        }
        let connected = sessanchor::daemon::request(&root, json!({"op":"connect","id":"dev"}))
            .map_err(str::to_owned)?;
        if connected["state"] != "available" {
            return Err(connected.to_string());
        }
        let start = Instant::now();
        loop {
            let status: Value = sessanchor::daemon::request(&root, json!({"op":"status"}))
                .map_err(str::to_owned)?;
            if status["devices"][0]["state"] == "sleeping" {
                break;
            }
            if start.elapsed() > Duration::from_secs(5) {
                return Err("never slept".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let count = std::fs::read_to_string(root.join("calls")).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(1200));
        if std::fs::read_to_string(root.join("calls")).map_err(|e| e.to_string())? != count {
            return Err("sleeping host probed".into());
        }
        Ok(())
    })();
    let _ = sessanchor::daemon::request(&root, json!({"op":"stop"}));
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(root).unwrap();
    assert!(result.is_ok(), "{result:?}");
}

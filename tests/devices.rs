#![cfg(unix)]
use serde_json::Value;
use std::{
    path::PathBuf,
    process::{Command, Output},
};

struct State(PathBuf);
impl State {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
                "sanc-device-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sanc"))
            .arg("--state-dir")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn device_config_survives_cli_restart_without_implying_connected() {
    let state = State::new();
    let key = state.0.join("key");
    let added = state.run(&[
        "device",
        "add",
        "sample",
        "--host",
        "example.test",
        "--user",
        "tester",
        "--identity",
        key.to_str().unwrap(),
    ]);
    assert!(added.status.success(), "{:?}", added);
    let listed = state.run(&["device", "list"]);
    assert!(listed.status.success());
    let data: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(data["devices"][0]["id"], "sample");
    assert_eq!(data["devices"][0]["current"], "unknown");
    assert_eq!(data["devices"][0]["connected_this_boot"], false);
    assert!(!state
        .run(&[
            "device",
            "add",
            "sample",
            "--host",
            "another.test",
            "--user",
            "tester",
            "--identity",
            key.to_str().unwrap()
        ])
        .status
        .success());
}

#[test]
fn rejects_world_readable_state_and_symlink() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let state = State::new();
    std::fs::create_dir(&state.0).unwrap();
    std::fs::set_permissions(&state.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!state.run(&["device", "list"]).status.success());
    std::fs::set_permissions(&state.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    let victim = state.0.join("untouched");
    std::fs::write(&victim, b"not a database").unwrap();
    symlink(&victim, state.0.join("state.sqlite3")).unwrap();
    assert!(!state.run(&["device", "list"]).status.success());
    assert_eq!(std::fs::read(victim).unwrap(), b"not a database");
}

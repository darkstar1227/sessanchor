#![cfg(unix)]
use serde_json::Value;
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
}
static FIXTURE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "sanc-exec-test-{}-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let mock = root.join("bin/ssh");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ssh"),
            &mock,
        )
        .unwrap();
        std::fs::set_permissions(mock, std::fs::Permissions::from_mode(0o700)).unwrap();
        let f = Self { root };
        f.ok(&[
            "device",
            "add",
            "dev",
            "--host",
            "example.test",
            "--user",
            "tester",
            "--identity",
            f.root.join("key").to_str().unwrap(),
        ]);
        f.ok(&["session", "create", "s", "--device", "dev"]);
        f
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sanc"));
        c.arg("--state-dir")
            .arg(self.root.join("state"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("SANC_TEST_CALLS", self.root.join("calls"));
        c
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.command().args(args).output().unwrap();
        assert!(o.status.success(), "{:?}", o);
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn wait(&self, id: &str) -> Value {
        let start = Instant::now();
        loop {
            let t = self.ok(&["task", id]);
            if ["exited", "unknown"].contains(&t["task"]["state"].as_str().unwrap()) {
                return t;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn worker_survives_submitter_and_retry_never_reexecutes() {
    let f = Fixture::new();
    let submitted = f.ok(&["exec", "s", "--request-id", "r", "--command", "echo test"]);
    let id = submitted["task"]["task_id"].to_string();
    let terminal = f.wait(&id);
    assert_eq!(terminal["task"]["exit_code"], 0);
    let again = f.ok(&["exec", "s", "--request-id", "r", "--command", "echo test"]);
    assert_eq!(again["task"]["task_id"], submitted["task"]["task_id"]);
    assert_eq!(
        std::fs::read_to_string(f.root.join("calls")).unwrap(),
        "called\n"
    );
    let output = f.ok(&["output", &id]);
    assert_eq!(output["encoding"], "utf8");
    assert_eq!(output["output"], "中文 output\n");
    let cursor = output["next_cursor"].to_string();
    assert_eq!(f.ok(&["output", &id, "--cursor", &cursor])["output"], "");
    let rejected = f
        .command()
        .args(["exec", "s", "--request-id", "sudo", "--command", " sudo id"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8(rejected.stderr)
        .unwrap()
        .contains("approval_required"));
}

#[test]
fn ssh_transport_failure_is_unknown_not_a_remote_exit_code() {
    let f = Fixture::new();
    let out = f
        .command()
        .env("SANC_TEST_EXIT", "255")
        .args(["exec", "s", "--request-id", "r", "--command", "id"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let task: Value = serde_json::from_slice(&out.stdout).unwrap();
    let terminal = f.wait(&task["task"]["task_id"].to_string());
    assert_eq!(terminal["task"]["state"], "unknown");
    assert!(terminal["task"]["exit_code"].is_null());
    let retry = f.ok(&["exec", "s", "--request-id", "r", "--command", "id"]);
    assert_eq!(retry["task"]["state"], "unknown");
    assert_eq!(
        std::fs::read_to_string(f.root.join("calls")).unwrap(),
        "called\n"
    );
}

#[test]
fn binary_output_is_lossless_and_bounded() {
    let f = Fixture::new();
    let task = f.ok(&["exec", "s", "--request-id", "binary", "--command", "id"]);
    let id = task["task"]["task_id"].to_string();
    f.wait(&id);
    let bytes = vec![255u8; 5000];
    std::fs::write(f.root.join(format!("state/task-{id}-stdout.log")), &bytes).unwrap();
    let page = f.ok(&["output", &id]);
    assert_eq!(page["encoding"], "hex");
    assert_eq!(page["output"].as_str().unwrap().len(), 4096);
    assert_eq!(page["next_cursor"], 2048);
    assert_eq!(page["more"], true);
}

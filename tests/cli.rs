use std::process::Command;

#[test]
fn capabilities_disclose_unimplemented_transport() {
    let out = Command::new(env!("CARGO_BIN_EXE_sanc"))
        .arg("capabilities")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"ssh\":false"));
    assert!(text.contains("\"durable_tasks\":false"));
    assert!(text.len() < 512);
}

#[test]
fn unsupported_commands_fail_without_echoing_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_sanc"))
        .args(["exec", "secret-marker"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let text = String::from_utf8(out.stderr).unwrap();
    assert!(text.contains("unsupported_command"));
    assert!(!text.contains("secret-marker"));
}

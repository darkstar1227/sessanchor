use base64::{engine::general_purpose::STANDARD, Engine as _};
use sessanchor::ssh::remote_command;

#[test]
fn encodes_utf16_without_exposing_shell_metacharacters() {
    let raw = "Write-Output '中文 \" & | $x'\nexit 7";
    let command = remote_command(raw, "powershell").unwrap();
    let encoded = command.split_whitespace().last().unwrap();
    let bytes = STANDARD.decode(encoded).unwrap();
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let script = String::from_utf16(&units).unwrap();
    assert!(script.contains(raw));
    assert!(!command.contains("中文"));
    assert!(script.contains("$sancOk = $?"));
    assert!(script.contains(&format!("\n{raw}\n$sancOk = $?")));
    assert!(!script.contains("& {"));
}

#[test]
fn rejects_sudo_invalid_shell_and_oversize_commands() {
    assert_eq!(
        remote_command(" sudo whoami", "powershell"),
        Err("approval_required")
    );
    assert_eq!(remote_command("echo ok", "bash"), Err("unsupported_shell"));
    assert!(remote_command(&"x".repeat(4000), "powershell").is_err());
    assert_eq!(remote_command("ver", "default").unwrap(), "ver");
}

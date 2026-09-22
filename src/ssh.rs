//! OpenSSH transport and explicit remote command encoding.
use base64::{engine::general_purpose::STANDARD, Engine as _};

pub fn remote_command(command: &str, shell: &str) -> Result<String, &'static str> {
    crate::check_command_policy(command).map_err(|_| "approval_required")?;
    match shell {
        "default" => Ok(command.to_owned()),
        "powershell" => {
            // Keep below the Windows default shell's command-line limit.
            // Capture status in the same scope: invoking a scriptblock can reset
            // $? to true even when its final cmdlet emitted a nonterminating error.
            let script = format!("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $OutputEncoding = [Console]::OutputEncoding; $global:LASTEXITCODE = 0;\n{command}\n$sancOk = $?; if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}; if (-not $sancOk) {{ exit 1 }}; exit 0");
            let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let encoded = STANDARD.encode(bytes);
            if encoded.len() > 7600 || command.contains('\0') {
                return Err("powershell_command_too_long_or_invalid");
            }
            Ok(format!(
                "powershell.exe -NoProfile -NonInteractive -EncodedCommand {encoded}"
            ))
        }
        _ => Err("unsupported_shell"),
    }
}
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Target {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity: PathBuf,
}

impl Target {
    pub fn command(&self) -> Result<Command, &'static str> {
        self.execution_command("exit 0")
    }

    pub fn execution_command(&self, remote: &str) -> Result<Command, &'static str> {
        crate::check_command_policy(remote).map_err(|_| "approval_required")?;
        if self.host.is_empty()
            || self.host.starts_with('-')
            || self.host.len() > 253
            || !self
                .host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-:_%".contains(&c))
            || self.user.is_empty()
            || self.user.starts_with('-')
            || self.user.len() > 128
            || !self
                .user
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || self.port == 0
            || !self.identity.is_absolute()
        {
            return Err("invalid_target");
        }
        let mut cmd = Command::new("ssh");
        cmd.args(["-F", "none", "-T", "-n"]);
        for option in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "UpdateHostKeys=no",
            "ConnectTimeout=8",
            "ConnectionAttempts=1",
            "ClearAllForwardings=yes",
            "ForwardAgent=no",
            "IdentitiesOnly=yes",
            "ControlMaster=no",
            "ControlPath=none",
        ] {
            cmd.args(["-o", option]);
        }
        cmd.arg("-i")
            .arg(&self.identity)
            .arg("-l")
            .arg(&self.user)
            .arg("-p")
            .arg(self.port.to_string())
            .arg(&self.host)
            .arg(remote);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(cmd)
    }
}

/// Success means a verified authenticated connection completed a shell probe.
/// Failure is deliberately not classified as offline: it may be auth/host-key.
pub fn probe(target: &Target) -> Result<(), &'static str> {
    let mut child = target.command()?.spawn().map_err(|_| "ssh_spawn_failed")?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err("ssh_probe_failed")
                }
            }
            Ok(None) if start.elapsed() < Duration::from_secs(15) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(if outcome.is_err() {
                    "ssh_wait_failed"
                } else {
                    "ssh_probe_timeout"
                });
            }
        }
    }
}

pub fn inspect_os(target: &Target, platform: &str) -> Result<(String, String), &'static str> {
    use std::io::Read;
    let command=match platform {
        "posix"=>"if test -r /etc/os-release; then . /etc/os-release; printf '%s\\n%s\\n' \"$NAME\" \"$VERSION_ID\"; elif command -v sw_vers >/dev/null 2>&1; then sw_vers -productName; sw_vers -productVersion; else uname -s; uname -r; fi",
        "windows"=>"powershell.exe -NoProfile -NonInteractive -Command \"[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $o = Get-CimInstance Win32_OperatingSystem; $o.Caption; $o.Version\"",
        _=>return Err("unsupported_os_query"),
    };
    let mut child = target
        .execution_command(command)?
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "ssh_spawn_failed")?;
    let stdout = child.stdout.take().ok_or("output_pipe_failed")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(8193).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < Duration::from_secs(15) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "os_query_failed")?
        .map_err(|_| "os_query_failed")?;
    if !status.is_some_and(|s| s.success()) || bytes.len() > 8192 {
        return Err("os_query_failed");
    }
    let text = String::from_utf8(bytes).map_err(|_| "os_encoding_invalid")?;
    let lines: Vec<_> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if lines.len() != 2 {
        return Err("os_response_invalid");
    }
    Ok((lines[0].trim().into(), lines[1].trim().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            host: "example.test".into(),
            user: "tester".into(),
            port: 22,
            identity: std::env::temp_dir().join("test-key"),
        }
    }
    #[test]
    fn probe_ignores_config_and_never_auto_trusts() {
        let cmd = target().command().unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..2], ["-F", "none"]);
        assert!(args.contains(&"StrictHostKeyChecking=yes".into()));
        assert!(args.contains(&"BatchMode=yes".into()));
        assert_eq!(args.last().unwrap(), "exit 0");
    }
    #[test]
    fn rejects_option_and_shell_injection() {
        for host in ["-oProxyCommand=bad", "user@host", "host;id", "$(id)", ""] {
            let mut t = target();
            t.host = host.into();
            assert!(t.command().is_err());
        }
        let mut t = target();
        t.user = "x;id".into();
        assert!(t.command().is_err());
    }
}

//! Initial OpenSSH-backed probe. No persistent transport or arbitrary execution.
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

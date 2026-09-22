//! Local worker keeps SSH alive after the submitting CLI exits.
//! Remote persistence across transport loss is NOT provided by this backend.
use crate::{connections::ConnectionStore, state};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn store(dir: &Path) -> Result<ConnectionStore, &'static str> {
    ConnectionStore::open(
        state::database_path(dir).map_err(|_| "unsafe_database_file")?,
        &state::boot_id().map_err(|_| "boot_identity_unavailable")?,
    )
    .map_err(|_| "database_open_failed")
}

pub fn launch(dir: &Path, id: i64) -> Result<(), &'static str> {
    let mut worker = Command::new(std::env::current_exe().map_err(|_| "executable_unavailable")?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        worker.process_group(0);
    }
    worker
        .arg("--state-dir")
        .arg(dir)
        .arg("worker")
        .arg(id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "worker_spawn_failed")?;
    Ok(())
}

fn output_path(dir: &Path, id: i64, stream: &str) -> Result<PathBuf, &'static str> {
    if id <= 0 || !["stdout", "stderr"].contains(&stream) {
        return Err("invalid_output_reference");
    }
    Ok(dir.join(format!("task-{id}-{stream}.log")))
}

fn create_output(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

// Drain both pipes even after storage limit; otherwise full pipes deadlock SSH.
// Retention integration is pending; capped output must be explicitly signalled.
fn capture(mut reader: impl Read, mut file: File) -> std::io::Result<bool> {
    let mut remaining = 64 * 1024 * 1024usize;
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let keep = n.min(remaining);
        file.write_all(&buffer[..keep])?;
        remaining -= keep;
        truncated |= keep < n;
    }
    file.sync_all()?;
    Ok(truncated)
}

pub fn execute(dir: &Path, id: i64) -> Result<(), &'static str> {
    let mut db = store(dir)?;
    let (target, command) = db.task_execution(id)?;
    crate::check_command_policy(&command).map_err(|_| "approval_required")?;
    if !db.claim_task(id)? {
        return Ok(());
    }
    let result = execute_claimed(dir, id, &target, &command);
    let exit = match result {
        Ok(Some(code)) if code != 255 => Some(code),
        _ => None,
    };
    db.finish_task(id, exit, state::now().map_err(|_| "clock_unavailable")?)?;
    result.map(|_| ())
}

fn execute_claimed(
    dir: &Path,
    id: i64,
    target: &crate::ssh::Target,
    remote: &str,
) -> Result<Option<i32>, &'static str> {
    let out =
        create_output(&output_path(dir, id, "stdout")?).map_err(|_| "output_create_failed")?;
    let err =
        create_output(&output_path(dir, id, "stderr")?).map_err(|_| "output_create_failed")?;
    let mut child = target
        .execution_command(remote)?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "ssh_spawn_failed")?;
    let stdout = child.stdout.take().ok_or("output_pipe_failed")?;
    let stderr = child.stderr.take().ok_or("output_pipe_failed")?;
    let a = std::thread::spawn(move || capture(stdout, out));
    let b = std::thread::spawn(move || capture(stderr, err));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(e) => break Err(e),
            Ok(None) => {
                let _ = store(dir).and_then(|mut db| db.heartbeat(id));
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    };
    let a = a
        .join()
        .map_err(|_| "output_capture_failed")?
        .map_err(|_| "output_capture_failed")?;
    let b = b
        .join()
        .map_err(|_| "output_capture_failed")?
        .map_err(|_| "output_capture_failed")?;
    if a || b {
        create_output(&dir.join(format!("task-{id}-output-truncated")))
            .map_err(|_| "output_capture_failed")?;
    }
    Ok(status.map_err(|_| "ssh_wait_failed")?.code())
}

pub fn output(
    dir: &Path,
    id: i64,
    stream: &str,
    cursor: u64,
) -> Result<serde_json::Value, &'static str> {
    let db = store(dir)?;
    let task = db.task(id)?;
    let path = output_path(dir, id, stream)?;
    if !path.exists() {
        if cursor != 0 {
            return Err("invalid_cursor");
        }
        return Ok(
            serde_json::json!({"task":task,"output":"","next_cursor":0,"output_pending":true}),
        );
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "output_read_failed")?;
    if !meta.is_file() || meta.file_type().is_symlink() || cursor > meta.len() {
        return Err("invalid_cursor_or_output");
    }
    let mut file = File::open(&path).map_err(|_| "output_read_failed")?;
    file.seek(SeekFrom::Start(cursor))
        .map_err(|_| "invalid_cursor")?;
    // UTF-8 when possible; bounded hex fallback preserves arbitrary binary bytes.
    let mut bytes = vec![0; 4096];
    let n = file.read(&mut bytes).map_err(|_| "output_read_failed")?;
    bytes.truncate(n);
    let (encoding, text, used) = match std::str::from_utf8(&bytes) {
        Ok(s) => ("utf8", s.to_owned(), n),
        Err(e) if e.error_len().is_none() && e.valid_up_to() > 0 => {
            let used = e.valid_up_to();
            (
                "utf8",
                String::from_utf8(bytes[..used].to_vec()).map_err(|_| "output_encoding_failed")?,
                used,
            )
        }
        Err(_) => {
            let used = n.min(2048);
            (
                "hex",
                bytes[..used]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                used,
            )
        }
    };
    Ok(
        serde_json::json!({"task":task,"stream":stream,"encoding":encoding,"output":text,"next_cursor":cursor+used as u64,"more":cursor+(used as u64)<meta.len(),"output_truncated":dir.join(format!("task-{id}-output-truncated")).exists()}),
    )
}

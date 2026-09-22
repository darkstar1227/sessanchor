//! Local monitoring daemon. Probe connections are short-lived, not multiplexed yet.
use serde_json::Value;
use std::path::Path;

#[cfg(not(unix))]
pub fn serve(_dir: &Path, _idle: u64, _interval: u64) -> Result<(), &'static str> {
    Err("daemon_platform_unsupported")
}
#[cfg(not(unix))]
pub fn request(_dir: &Path, _operation: Value) -> Result<Value, &'static str> {
    Err("daemon_platform_unsupported")
}

#[cfg(unix)]
pub use unix::{request, serve};

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::{connections::ConnectionState, ssh, state, worker};
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        fs::OpenOptions,
        io::{BufRead, BufReader, Read, Write},
        os::unix::{
            fs::{FileTypeExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
        },
        time::{Duration, Instant},
    };

    struct Device {
        last_use: Instant,
        last_probe: Instant,
        status: ConnectionState,
        busy: bool,
    }

    pub fn request(dir: &Path, operation: Value) -> Result<Value, &'static str> {
        let mut stream =
            UnixStream::connect(dir.join("daemon.sock")).map_err(|_| "daemon_unavailable")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .map_err(|_| "ipc_failed")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| "ipc_failed")?;
        writeln!(stream, "{operation}").map_err(|_| "ipc_failed")?;
        let mut text = String::new();
        BufReader::new(stream)
            .take(1024 * 1024)
            .read_line(&mut text)
            .map_err(|_| "ipc_failed")?;
        serde_json::from_str(&text).map_err(|_| "ipc_invalid_response")
    }

    pub fn serve(dir: &Path, idle: u64, interval: u64) -> Result<(), &'static str> {
        if interval == 0 {
            return Err("invalid_probe_interval");
        }
        let lock_path = dir.join("daemon.lock");
        if let Ok(meta) = std::fs::symlink_metadata(&lock_path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("unsafe_lock_file");
            }
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)
            .map_err(|_| "daemon_lock_failed")?;
        lock.try_lock().map_err(|_| "daemon_already_running")?;
        let socket = dir.join("daemon.sock");
        if let Ok(meta) = std::fs::symlink_metadata(&socket) {
            if !meta.file_type().is_socket() {
                return Err("unsafe_socket_path");
            }
            std::fs::remove_file(&socket).map_err(|_| "socket_cleanup_failed")?;
        }
        let listener = UnixListener::bind(&socket).map_err(|_| "socket_bind_failed")?;
        listener.set_nonblocking(true).map_err(|_| "ipc_failed")?;
        let mut db = worker::store(dir)?;
        let mut devices = BTreeMap::<String, Device>::new();
        for d in db.devices().map_err(|_| "database_read_failed")? {
            if d.connected_this_boot {
                devices.insert(
                    d.id,
                    Device {
                        last_use: Instant::now(),
                        last_probe: Instant::now() - Duration::from_secs(interval),
                        status: ConnectionState::Unknown,
                        busy: false,
                    },
                );
            }
        }
        let mut stop = false;
        while !stop {
            // IPC gets priority over periodic work. Reads have bounded deadlines.
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                let mut text = String::new();
                if BufReader::new(&mut stream)
                    .take(16385)
                    .read_line(&mut text)
                    .is_ok()
                    && text.len() <= 16384
                {
                    if let Ok(op) = serde_json::from_str::<Value>(&text) {
                        let result = match op["op"].as_str().unwrap_or("") {
                            "status" => {
                                json!({"daemon":"running","persistent_connections":false,"devices":devices.iter().map(|(id,d)|json!({"device_id":id,"state":d.status,"idle_seconds":d.last_use.elapsed().as_secs()})).collect::<Vec<_>>()})
                            }
                            "stop" => {
                                stop = true;
                                json!({"daemon":"stopping"})
                            }
                            "connect" => {
                                let id = op["id"].as_str().unwrap_or("");
                                match db.target(id) {
                                    Ok(target) => {
                                        let result = ssh::probe(&target);
                                        let status = if result.is_ok() {
                                            ConnectionState::Available
                                        } else {
                                            ConnectionState::Unknown
                                        };
                                        let saved = db.observe(
                                            id,
                                            status,
                                            state::now().map_err(|_| "clock_unavailable")?,
                                            result.as_ref().err().copied(),
                                        );
                                        if saved.is_err() {
                                            json!({"error":"observation_save_failed"})
                                        } else {
                                            if result.is_ok() {
                                                devices.insert(
                                                    id.into(),
                                                    Device {
                                                        last_use: Instant::now(),
                                                        last_probe: Instant::now(),
                                                        status,
                                                        busy: false,
                                                    },
                                                );
                                            }
                                            json!({"device_id":id,"state":status,"error":result.err(),"persistent":false})
                                        }
                                    }
                                    Err(_) => json!({"error":"device_not_configured"}),
                                }
                            }
                            _ => json!({"error":"invalid_operation"}),
                        };
                        let _ = writeln!(stream, "{result}");
                    }
                }
            }
            for (id, d) in &mut devices {
                let busy = db.device_busy(id).map_err(|_| "database_read_failed")?;
                if d.busy && !busy {
                    d.last_use = Instant::now();
                }
                d.busy = busy;
                if d.status == ConnectionState::Sleeping && !busy {
                    continue;
                }
                if idle > 0 && !busy && d.last_use.elapsed() >= Duration::from_secs(idle) {
                    d.status = ConnectionState::Sleeping;
                    db.observe(
                        id,
                        d.status,
                        state::now().map_err(|_| "clock_unavailable")?,
                        None,
                    )
                    .map_err(|_| "observation_save_failed")?;
                    continue;
                }
                if d.last_probe.elapsed() >= Duration::from_secs(interval) {
                    if let Ok(target) = db.target(id) {
                        let result = ssh::probe(&target);
                        d.status = if result.is_ok() {
                            ConnectionState::Available
                        } else {
                            ConnectionState::Unknown
                        };
                        db.observe(
                            id,
                            d.status,
                            state::now().map_err(|_| "clock_unavailable")?,
                            result.as_ref().err().copied(),
                        )
                        .map_err(|_| "observation_save_failed")?;
                        d.last_probe = Instant::now();
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        std::fs::remove_file(socket).map_err(|_| "socket_cleanup_failed")?;
        drop(lock);
        Ok(())
    }
}

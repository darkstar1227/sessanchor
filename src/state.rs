//! Private local state directory. Windows ACL integration is not implemented yet.
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn directory(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    let path = explicit
        .or_else(|| std::env::var_os("SESSANCHOR_STATE_DIR").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state/sessanchor"))
        })
        .ok_or_else(|| io::Error::other("state directory required"))?;
    if !path.is_absolute() {
        return Err(io::Error::other("state directory must be absolute"));
    }
    secure_directory(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !path.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "state directory must be private, non-symlink directory (0700)",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::other(
        "secure state ACL support is not implemented on this platform",
    ))
}

pub fn database_path(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join("state.sqlite3");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_file()
            || meta.file_type().is_symlink()
            || meta.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::other("unsafe database file"));
        }
    }
    Ok(path)
}

pub fn boot_id() -> io::Result<String> {
    #[cfg(target_os = "linux")]
    let value = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    #[cfg(target_os = "macos")]
    let value = {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.bootsessionuuid"])
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other("boot identity unavailable"));
        }
        String::from_utf8(out.stdout).map_err(io::Error::other)?
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let value = String::new();
    if value.trim().is_empty() {
        return Err(io::Error::other("boot identity unavailable"));
    }
    Ok(value.trim().into())
}

pub fn now() -> io::Result<i64> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    i64::try_from(seconds).map_err(io::Error::other)
}

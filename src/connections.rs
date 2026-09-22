//! Persisted observations are history, never proof of a live SSH connection.
use rusqlite::{params, Connection, Result};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Unknown,
    Available,
    Unavailable,
    Reconnecting,
    AwaitingAuth,
    Sleeping,
}

impl ConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Reconnecting => "reconnecting",
            Self::AwaitingAuth => "awaiting_auth",
            Self::Sleeping => "sleeping",
        }
    }
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct DeviceView {
    pub id: String,
    pub address: String,
    pub current: ConnectionState,
    pub last_observed_state: Option<String>,
    pub last_checked_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_reason_code: Option<String>,
    pub connected_this_boot: bool,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_updated_at: Option<i64>,
    pub pinned: bool,
}

/// A single future daemon owns this object. Opening a store does not probe hosts.
/// The caller supplies an OS boot identity (not a process-start UUID).
/// Caller must protect the database directory with OS permissions.
pub struct ConnectionStore {
    pub(crate) db: Connection,
    pub(crate) boot_id: String,
    live: BTreeMap<String, ConnectionState>,
}

impl ConnectionStore {
    pub fn device_busy(&self, id: &str) -> Result<bool> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM tasks t JOIN sessions s ON s.id=t.session_id WHERE s.device_id=?1 AND t.state!='exited')",[id],|r|r.get(0))
    }
    pub fn open(path: impl AsRef<Path>, boot_id: &str) -> Result<Self> {
        if boot_id.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "empty boot identity".into(),
            ));
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 3 {
            return Err(rusqlite::Error::InvalidParameterName(
                "unsupported database version".into(),
            ));
        }
        if version == 0 {
            let tx = db.transaction()?;
            tx.execute_batch(
                "CREATE TABLE devices (
                    id TEXT PRIMARY KEY CHECK(length(id)>0),
                    address TEXT NOT NULL CHECK(length(address)>0),
                    last_state TEXT,
                    last_checked_at INTEGER,
                    last_success_at INTEGER,
                    last_reason_code TEXT,
                    success_boot_id TEXT
                );
                CREATE TABLE connection_events (
                    sequence INTEGER PRIMARY KEY,
                    device_id TEXT NOT NULL REFERENCES devices(id),
                    boot_id TEXT NOT NULL,
                    observed_at INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    reason_code TEXT
                );
                PRAGMA user_version = 1;",
            )?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = db.transaction()?;
            tx.execute_batch(
                "ALTER TABLE devices ADD COLUMN login_user TEXT;
                ALTER TABLE devices ADD COLUMN port INTEGER;
                ALTER TABLE devices ADD COLUMN identity_path TEXT;
                PRAGMA user_version = 2;",
            )?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = db.transaction()?;
            tx.execute_batch(
                "ALTER TABLE devices ADD COLUMN os_name TEXT;
                ALTER TABLE devices ADD COLUMN os_version TEXT;
                ALTER TABLE devices ADD COLUMN os_updated_at INTEGER;
                ALTER TABLE devices ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
                PRAGMA user_version=3;",
            )?;
            tx.commit()?;
        }
        crate::tasks::initialize(&db)?;
        db.execute("UPDATE tasks SET state='unknown' WHERE state='running' AND (worker_boot IS NULL OR worker_boot!=?1 OR lease_at IS NULL OR lease_at<unixepoch()-30)",[boot_id])?;
        Ok(Self {
            db,
            boot_id: boot_id.into(),
            live: BTreeMap::new(),
        })
    }

    /// Insert only: endpoint changes require a separate identity/reset policy.
    /// Address must be a hostname/IP, never a URL containing credentials.
    pub fn add_device(&mut self, id: &str, address: &str) -> Result<()> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || address.is_empty()
            || address.len() > 253
            || !address
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-:[]%_".contains(&c))
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid device identity or address".into(),
            ));
        }
        self.db.execute(
            "INSERT INTO devices(id,address) VALUES (?1,?2)",
            params![id, address],
        )?;
        Ok(())
    }

    pub fn configure(&mut self, id: &str, target: &crate::ssh::Target) -> Result<()> {
        target
            .command()
            .map_err(|_| rusqlite::Error::InvalidParameterName("invalid target".into()))?;
        let identity = target.identity.to_str().ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("identity path must be UTF-8".into())
        })?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid device ID".into(),
            ));
        }
        self.db.execute(
            "INSERT INTO devices(id,address,login_user,port,identity_path) VALUES (?1,?2,?3,?4,?5)",
            params![id, target.host, target.user, target.port, identity],
        )?;
        Ok(())
    }

    pub fn target(&self, id: &str) -> Result<crate::ssh::Target> {
        self.db.query_row(
            "SELECT address,login_user,port,identity_path FROM devices WHERE id=?1",
            [id],
            |r| {
                Ok(crate::ssh::Target {
                    host: r.get(0)?,
                    user: r.get(1)?,
                    port: r.get(2)?,
                    identity: std::path::PathBuf::from(r.get::<_, String>(3)?),
                })
            },
        )
    }

    pub fn events(&self, id: &str, after: i64) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.db.prepare("SELECT sequence,observed_at,state,reason_code FROM connection_events WHERE device_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 50")?;
        let rows = stmt.query_map(params![id,after], |r| Ok(serde_json::json!({
            "cursor": r.get::<_,i64>(0)?, "at":r.get::<_,i64>(1)?, "state":r.get::<_,String>(2)?, "reason":r.get::<_,Option<String>>(3)?
        })))?;
        rows.collect()
    }

    /// Receives sanitized reason codes, not raw SSH stderr or authentication data.
    /// Snapshot and audit event commit together; memory changes only after commit.
    pub fn observe(
        &mut self,
        id: &str,
        state: ConnectionState,
        at: i64,
        reason: Option<&str>,
    ) -> Result<()> {
        if at < 0
            || reason.is_some_and(|s| {
                s.len() > 64
                    || s.is_empty()
                    || !s
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            })
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid observation".into(),
            ));
        }
        let tx = self.db.transaction()?;
        let updated = tx.execute(
            "UPDATE devices SET last_state=?2,last_checked_at=?3,last_reason_code=?4,
             last_success_at=CASE WHEN ?2='available' THEN ?3 ELSE last_success_at END,
             success_boot_id=CASE WHEN ?2='available' THEN ?5 ELSE success_boot_id END
             WHERE id=?1 AND (last_checked_at IS NULL OR last_checked_at<=?3)",
            params![id, state.as_str(), at, reason, self.boot_id],
        )?;
        if updated != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.execute("INSERT INTO connection_events(device_id,boot_id,observed_at,state,reason_code) VALUES (?1,?2,?3,?4,?5)", params![id,self.boot_id,at,state.as_str(),reason])?;
        tx.commit()?;
        self.live.insert(id.into(), state);
        Ok(())
    }

    pub fn save_os(&mut self, id: &str, name: &str, version: &str, at: i64) -> Result<()> {
        if name.is_empty()
            || name.len() > 256
            || version.is_empty()
            || version.len() > 256
            || name.chars().chain(version.chars()).any(char::is_control)
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid OS metadata".into(),
            ));
        }
        let n = self.db.execute(
            "UPDATE devices SET os_name=?2,os_version=?3,os_updated_at=?4 WHERE id=?1",
            params![id, name, version, at],
        )?;
        if n != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    pub fn pin(&mut self, id: &str, pinned: bool) -> Result<()> {
        let n = self.db.execute(
            "UPDATE devices SET pinned=?2 WHERE id=?1",
            params![id, pinned],
        )?;
        if n != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    /// Cached list only. On every reopen current state is Unknown for all devices.
    pub fn devices(&self) -> Result<Vec<DeviceView>> {
        let mut stmt = self.db.prepare("SELECT id,address,last_state,last_checked_at,last_success_at,last_reason_code,success_boot_id,os_name,os_version,os_updated_at,pinned FROM devices ORDER BY pinned DESC,last_success_at DESC,id")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let boot: Option<String> = row.get(6)?;
            Ok(DeviceView {
                current: self
                    .live
                    .get(&id)
                    .copied()
                    .unwrap_or(ConnectionState::Unknown),
                id,
                address: row.get(1)?,
                last_observed_state: row.get(2)?,
                last_checked_at: row.get(3)?,
                last_success_at: row.get(4)?,
                last_reason_code: row.get(5)?,
                connected_this_boot: boot.as_deref() == Some(&self.boot_id),
                os_name: row.get(7)?,
                os_version: row.get(8)?,
                os_updated_at: row.get(9)?,
                pinned: row.get(10)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_are_atomic_and_keep_last_success() {
        let mut store = ConnectionStore::open(":memory:", "boot-a").unwrap();
        store.add_device("dev", "192.0.2.1").unwrap();
        store
            .observe("dev", ConnectionState::Available, 10, None)
            .unwrap();
        store
            .observe("dev", ConnectionState::Unavailable, 11, Some("timeout"))
            .unwrap();
        assert!(store
            .observe("missing", ConnectionState::Available, 12, None)
            .is_err());
        assert!(store
            .observe("dev", ConnectionState::Available, 9, None)
            .is_err());
        let devices = store.devices().unwrap();
        assert_eq!(devices[0].current, ConnectionState::Unavailable);
        assert_eq!(devices[0].last_success_at, Some(10));
        assert!(devices[0].connected_this_boot);
        let count: i64 = store
            .db
            .query_row("SELECT count(*) FROM connection_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn disk_reopen_does_not_resurrect_connection() {
        let path = std::env::temp_dir().join(format!(
            "sessanchor-test-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut store = ConnectionStore::open(&path, "boot-a").unwrap();
            store.add_device("dev", "example.test").unwrap();
            store
                .observe("dev", ConnectionState::Available, 10, None)
                .unwrap();
        }
        for (boot, eligible) in [("boot-a", true), ("boot-b", false)] {
            let store = ConnectionStore::open(&path, boot).unwrap();
            let devices = store.devices().unwrap();
            assert_eq!(devices[0].current, ConnectionState::Unknown);
            assert_eq!(devices[0].last_observed_state.as_deref(), Some("available"));
            assert_eq!(devices[0].connected_this_boot, eligible);
        }
        std::fs::remove_file(path).unwrap();
    }
}

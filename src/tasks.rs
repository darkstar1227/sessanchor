//! Durable intent journal. A claimed task is never automatically re-dispatched.
use crate::{check_command_policy, connections::ConnectionStore};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

pub(crate) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id),
        description TEXT NOT NULL DEFAULT '', description_at INTEGER,
        user_controlled INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS tasks (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL REFERENCES sessions(id),
        request_id TEXT NOT NULL,
        command TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN ('accepted','running','unknown','exited')),
        created_at INTEGER NOT NULL, finished_at INTEGER,
        exit_code INTEGER,
        UNIQUE(session_id,request_id)
    );",
    )?;
    let mut stmt = db.prepare("PRAGMA table_info(tasks)")?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !names.iter().any(|n| n == "lease_at") {
        db.execute_batch("ALTER TABLE tasks ADD COLUMN lease_at INTEGER; ALTER TABLE tasks ADD COLUMN worker_boot TEXT;")?;
    }
    let mut stmt = db.prepare("PRAGMA table_info(sessions)")?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !names.iter().any(|n| n == "control_token") {
        db.execute_batch("ALTER TABLE sessions ADD COLUMN control_token TEXT; ALTER TABLE sessions ADD COLUMN control_lease INTEGER;")?;
    }
    db.execute("UPDATE sessions SET user_controlled=0,control_token=NULL,control_lease=NULL WHERE user_controlled=1 AND control_lease<unixepoch()-30",[])?;
    Ok(())
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TaskView {
    pub task_id: i64,
    pub session_id: String,
    pub request_id: String,
    pub state: String,
    pub exit_code: Option<i32>,
}

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskView> {
    Ok(TaskView {
        task_id: row.get(0)?,
        session_id: row.get(1)?,
        request_id: row.get(2)?,
        state: row.get(3)?,
        exit_code: row.get(4)?,
    })
}
const VIEW: &str = "SELECT id,session_id,request_id,state,exit_code FROM tasks";

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}

impl ConnectionStore {
    pub fn take_session(&mut self, id: &str, token: &str) -> Result<(), &'static str> {
        if token.len() < 32 {
            return Err("invalid_control_token");
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "database_busy")?;
        let n=tx.execute("UPDATE sessions SET user_controlled=1,control_token=?2,control_lease=unixepoch() WHERE id=?1 AND user_controlled=0 AND NOT EXISTS(SELECT 1 FROM tasks WHERE session_id=?1 AND state!='exited')",params![id,token]).map_err(|_| "database_write_failed")?;
        if n != 1 {
            return Err("session_busy_or_controlled");
        }
        tx.commit().map_err(|_| "database_write_failed")?;
        Ok(())
    }

    pub fn release_session(&mut self, id: &str, token: &str) -> Result<(), &'static str> {
        let n=self.db.execute("UPDATE sessions SET user_controlled=0,control_token=NULL,control_lease=NULL WHERE id=?1 AND control_token=?2",params![id,token]).map_err(|_| "database_write_failed")?;
        if n != 1 {
            return Err("control_not_owned");
        }
        Ok(())
    }

    pub fn renew_session(&mut self, id: &str, token: &str) -> Result<(), &'static str> {
        let n=self.db.execute("UPDATE sessions SET control_lease=unixepoch() WHERE id=?1 AND control_token=?2 AND user_controlled=1",params![id,token]).map_err(|_| "database_write_failed")?;
        if n != 1 {
            return Err("control_not_owned");
        }
        Ok(())
    }
    pub fn heartbeat(&mut self, id: i64) -> Result<(), &'static str> {
        self.db.execute("UPDATE tasks SET lease_at=unixepoch() WHERE id=?1 AND worker_boot=?2 AND state='running'",params![id,self.boot_id]).map_err(|_| "heartbeat_failed")?;
        Ok(())
    }
    pub fn task_execution(&self, id: i64) -> Result<(crate::ssh::Target, String), &'static str> {
        let (device,command):(String,String)=self.db.query_row("SELECT s.device_id,t.command FROM tasks t JOIN sessions s ON s.id=t.session_id WHERE t.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_| "task_not_found")?;
        Ok((
            self.target(&device).map_err(|_| "device_not_configured")?,
            command,
        ))
    }
    pub fn create_session(&mut self, id: &str, device: &str) -> Result<(), &'static str> {
        if !valid_id(id) {
            return Err("invalid_session_id");
        }
        self.target(device).map_err(|_| "device_not_configured")?;
        self.db
            .execute(
                "INSERT INTO sessions(id,device_id) VALUES (?1,?2)",
                params![id, device],
            )
            .map_err(|_| "session_create_failed")?;
        Ok(())
    }

    pub fn describe_session(
        &mut self,
        id: &str,
        description: &str,
        at: i64,
    ) -> Result<(), &'static str> {
        if description.len() > 1024 {
            return Err("description_too_long");
        }
        let n = self
            .db
            .execute(
                "UPDATE sessions SET description=?2,description_at=?3 WHERE id=?1",
                params![id, description, at],
            )
            .map_err(|_| "database_write_failed")?;
        if n == 0 {
            return Err("session_not_found");
        }
        Ok(())
    }

    pub fn sessions(&self) -> rusqlite::Result<Vec<serde_json::Value>> {
        let mut stmt=self.db.prepare("SELECT id,device_id,description,description_at,user_controlled FROM sessions ORDER BY id LIMIT 100")?;
        let rows=stmt.query_map([],|r|Ok(serde_json::json!({"session_id":r.get::<_,String>(0)?,"device_id":r.get::<_,String>(1)?,"description":r.get::<_,String>(2)?,"description_at":r.get::<_,Option<i64>>(3)?,"user_controlled":r.get::<_,bool>(4)?})))?;
        rows.collect()
    }

    pub fn reserve_task(
        &mut self,
        session: &str,
        request: &str,
        command: &str,
        at: i64,
    ) -> Result<TaskView, &'static str> {
        check_command_policy(command).map_err(|_| "approval_required")?;
        if !valid_id(request)
            || command.trim().is_empty()
            || command.len() > 65536
            || command.contains('\0')
        {
            return Err("invalid_request");
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "database_busy")?;
        let old: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,command FROM tasks WHERE session_id=?1 AND request_id=?2",
                params![session, request],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| "database_read_failed")?;
        let id = if let Some((id, old_command)) = old {
            if old_command != command {
                return Err("request_conflict");
            }
            id
        } else {
            let controlled: bool = tx
                .query_row(
                    "SELECT user_controlled FROM sessions WHERE id=?1",
                    [session],
                    |r| r.get(0),
                )
                .map_err(|_| "session_not_found")?;
            if controlled {
                return Err("user_controlled");
            }
            let busy: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tasks WHERE session_id=?1 AND state!='exited')",
                    [session],
                    |r| r.get(0),
                )
                .map_err(|_| "database_read_failed")?;
            if busy {
                return Err("session_busy");
            }
            tx.execute("INSERT INTO tasks(session_id,request_id,command,state,created_at) VALUES (?1,?2,?3,'accepted',?4)",params![session,request,command,at]).map_err(|_| "database_write_failed")?;
            tx.last_insert_rowid()
        };
        let task = tx
            .query_row(&format!("{VIEW} WHERE id=?1"), [id], task_row)
            .map_err(|_| "database_read_failed")?;
        tx.commit().map_err(|_| "database_write_failed")?;
        Ok(task)
    }

    pub fn task(&self, id: i64) -> Result<TaskView, &'static str> {
        self.db
            .query_row(&format!("{VIEW} WHERE id=?1"), [id], task_row)
            .map_err(|_| "task_not_found")
    }

    /// Atomic single dispatch claim; persistent running/unknown must not retry.
    pub fn claim_task(&mut self, id: i64) -> Result<bool, &'static str> {
        let n = self
            .db
            .execute(
                "UPDATE tasks SET state='running',lease_at=unixepoch(),worker_boot=?2 WHERE id=?1 AND state='accepted'",
                params![id,self.boot_id],
            )
            .map_err(|_| "database_write_failed")?;
        Ok(n == 1)
    }

    pub fn finish_task(&mut self, id: i64, exit: Option<i32>, at: i64) -> Result<(), &'static str> {
        let state = if exit.is_some() { "exited" } else { "unknown" };
        let n=self.db.execute("UPDATE tasks SET state=?2,exit_code=?3,finished_at=?4 WHERE id=?1 AND state IN ('running','unknown')",params![id,state,exit,at]).map_err(|_| "database_write_failed")?;
        if n != 1 {
            return Err("invalid_task_transition");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> ConnectionStore {
        let mut s = ConnectionStore::open(":memory:", "boot").unwrap();
        s.configure(
            "dev",
            &crate::ssh::Target {
                host: "example.test".into(),
                user: "tester".into(),
                port: 22,
                identity: std::env::temp_dir().join("key"),
            },
        )
        .unwrap();
        s.create_session("s", "dev").unwrap();
        s
    }
    #[test]
    fn durable_intent_conflicts_and_one_claim() {
        let mut s = store();
        let t = s.reserve_task("s", "r", "echo ok", 1).unwrap();
        assert_eq!(s.reserve_task("s", "r", "echo ok", 2).unwrap(), t);
        assert_eq!(
            s.reserve_task("s", "r", "other", 2),
            Err("request_conflict")
        );
        assert!(s.claim_task(t.task_id).unwrap());
        assert!(!s.claim_task(t.task_id).unwrap());
        s.finish_task(t.task_id, None, 3).unwrap();
        assert_eq!(
            s.reserve_task("s", "r", "echo ok", 4).unwrap().state,
            "unknown"
        );
        assert_eq!(
            s.reserve_task("s", "new", "echo ok", 4),
            Err("session_busy")
        );
        assert!(!s.claim_task(t.task_id).unwrap());
    }
    #[test]
    fn sudo_never_creates_intent() {
        let mut s = store();
        assert_eq!(
            s.reserve_task("s", "r", " sudo id", 1),
            Err("approval_required")
        );
        assert_eq!(s.reserve_task("s", "r", "id", 1).unwrap().task_id, 1);
    }

    #[test]
    fn takeover_is_exclusive_and_cannot_clear_unknown_tasks() {
        let mut s = store();
        let token = "12345678901234567890123456789012";
        s.take_session("s", token).unwrap();
        assert_eq!(s.reserve_task("s", "r", "id", 1), Err("user_controlled"));
        assert_eq!(s.release_session("s", "wrong"), Err("control_not_owned"));
        s.release_session("s", token).unwrap();
        let task = s.reserve_task("s", "r", "id", 1).unwrap();
        s.claim_task(task.task_id).unwrap();
        s.finish_task(task.task_id, None, 2).unwrap();
        assert_eq!(
            s.take_session("s", token),
            Err("session_busy_or_controlled")
        );
    }

    #[test]
    fn cross_connection_claim_and_reboot_preserve_dedup() {
        let path = std::env::temp_dir().join(format!(
            "sanc-journal-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut a = ConnectionStore::open(&path, "boot-a").unwrap();
            a.configure(
                "dev",
                &crate::ssh::Target {
                    host: "example.test".into(),
                    user: "tester".into(),
                    port: 22,
                    identity: std::env::temp_dir().join("key"),
                },
            )
            .unwrap();
            a.create_session("s", "dev").unwrap();
            let first = a.reserve_task("s", "r", "id", 1).unwrap();
            let mut b = ConnectionStore::open(&path, "boot-a").unwrap();
            assert!(a.claim_task(first.task_id).unwrap());
            assert!(!b.claim_task(first.task_id).unwrap());
        }
        {
            let mut recovered = ConnectionStore::open(&path, "boot-b").unwrap();
            let retry = recovered.reserve_task("s", "r", "id", 2).unwrap();
            assert_eq!(retry.state, "unknown");
            assert!(!recovered.claim_task(retry.task_id).unwrap());
        }
        std::fs::remove_file(path).unwrap();
    }
}

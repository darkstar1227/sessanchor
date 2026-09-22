//! Transport-independent contracts. Task execution remains in-memory only.
pub mod connections;
pub mod daemon;
pub mod ssh;
pub mod state;
pub mod tasks;
pub mod worker;
use std::collections::BTreeMap;

pub const DEFAULT_IDLE_SECONDS: u64 = 7200;
pub const DEFAULT_OUTPUT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    ApprovalRequired,
    InvalidRequest,
    RequestConflict,
    UserControlled,
    Busy,
    UnknownTask,
    InvalidTransition,
    InvalidCursor,
    InvalidBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Unknown,
    Exited(i32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub execution: Execution,
    pub state: TaskState,
}

#[derive(Debug, Default)]
pub struct Session {
    requests: BTreeMap<String, Task>,
    user_controlled: bool,
}

impl Session {
    /// Reserves an operation; does NOT execute it. IDs are session-local.
    /// The caller must serialize access and persist intent before dispatch.
    pub fn submit(&mut self, request_id: &str, execution: Execution) -> Result<&Task, Error> {
        check_command_policy(&execution.command)?;
        if request_id.is_empty() || execution.command.is_empty() {
            return Err(Error::InvalidRequest);
        }
        if let Some(task) = self.requests.get(request_id) {
            if task.execution != execution {
                return Err(Error::RequestConflict);
            }
            // Retrieving an existing request is read-only, even during takeover.
        } else {
            if self.user_controlled {
                return Err(Error::UserControlled);
            }
            if self.busy() {
                return Err(Error::Busy);
            }
            let id = self.requests.len() as u64 + 1;
            self.requests.insert(
                request_id.to_owned(),
                Task {
                    id,
                    execution,
                    state: TaskState::Running,
                },
            );
        }
        Ok(&self.requests[request_id])
    }

    pub fn observe(&mut self, request_id: &str, state: TaskState) -> Result<(), Error> {
        let task = self
            .requests
            .get_mut(request_id)
            .ok_or(Error::UnknownTask)?;
        if matches!(task.state, TaskState::Exited(_)) && task.state != state {
            return Err(Error::InvalidTransition);
        }
        task.state = state;
        Ok(())
    }

    pub fn busy(&self) -> bool {
        self.requests
            .values()
            .any(|t| !matches!(t.state, TaskState::Exited(_)))
    }

    pub fn take_control(&mut self) -> Result<(), Error> {
        if self.user_controlled {
            return Err(Error::UserControlled);
        }
        if self.busy() {
            return Err(Error::Busy);
        }
        self.user_controlled = true;
        Ok(())
    }

    /// Called only by the eventual authenticated ownership layer.
    pub fn release_control(&mut self) {
        self.user_controlled = false;
    }
}

/// A literal guard, not a shell parser or a complete privilege boundary.
/// No override: the agent must stop and ask the user, never rewrite or retry.
pub fn check_command_policy(command: &str) -> Result<(), Error> {
    let command = command.trim_start();
    if let Some(rest) = command.strip_prefix("sudo") {
        if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || ";|&<>()".contains(c))
        {
            return Err(Error::ApprovalRequired);
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct OutputPage<'a> {
    pub text: &'a str,
    pub next_cursor: usize,
    pub truncated: bool,
}

/// Byte cursors into one immutable UTF-8 stream. Binary decoding is out of scope.
pub fn output_page(text: &str, cursor: usize, budget: usize) -> Result<OutputPage<'_>, Error> {
    if !text.is_char_boundary(cursor) {
        return Err(Error::InvalidCursor);
    }
    if !(4..=DEFAULT_OUTPUT_BYTES).contains(&budget) {
        return Err(Error::InvalidBudget);
    }
    let mut end = cursor.saturating_add(budget).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(OutputPage {
        text: &text[cursor..end],
        next_cursor: end,
        truncated: end < text.len(),
    })
}

/// Caller supplies monotonic elapsed time since last use or active-work completion.
/// Health probes must not reset elapsed time. Unknown work blocks sleep too.
pub fn should_sleep(idle_seconds: u64, timeout: Option<u64>, active_or_unknown_work: bool) -> bool {
    !active_or_unknown_work && timeout.is_some_and(|limit| idle_seconds >= limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(value: &str) -> Execution {
        Execution {
            command: value.into(),
            cwd: None,
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn retries_reuse_task_and_reject_changed_parameters() {
        let mut s = Session::default();
        assert_eq!(s.submit("r", command("echo ok")).unwrap().id, 1);
        assert_eq!(s.submit("r", command("echo ok")).unwrap().id, 1);
        assert_eq!(
            s.submit("r", command("different")),
            Err(Error::RequestConflict)
        );
        let mut changed = command("echo ok");
        changed.cwd = Some("/tmp".into());
        assert_eq!(s.submit("r", changed), Err(Error::RequestConflict));
    }

    #[test]
    fn unknown_is_not_idle_and_retry_does_not_restart() {
        let mut s = Session::default();
        s.submit("r", command("work")).unwrap();
        s.observe("r", TaskState::Unknown).unwrap();
        assert_eq!(s.take_control(), Err(Error::Busy));
        assert_eq!(s.submit("new", command("work")), Err(Error::Busy));
        assert_eq!(
            s.submit("r", command("work")).unwrap().state,
            TaskState::Unknown
        );
        s.observe("r", TaskState::Exited(0)).unwrap();
        assert!(s.take_control().is_ok());
        assert_eq!(s.submit("new", command("work")), Err(Error::UserControlled));
        assert_eq!(
            s.submit("r", command("work")).unwrap().state,
            TaskState::Exited(0)
        );
        s.release_control();
        assert_eq!(s.submit("new", command("work")).unwrap().id, 2);
    }

    #[test]
    fn terminal_states_cannot_be_rewritten() {
        let mut s = Session::default();
        s.submit("r", command("work")).unwrap();
        s.observe("r", TaskState::Exited(3)).unwrap();
        assert_eq!(
            s.observe("r", TaskState::Running),
            Err(Error::InvalidTransition)
        );
        assert_eq!(
            s.observe("missing", TaskState::Unknown),
            Err(Error::UnknownTask)
        );
    }

    #[test]
    fn sessions_are_independent() {
        let mut a = Session::default();
        let mut b = Session::default();
        a.take_control().unwrap();
        assert!(b.submit("r", command("work")).is_ok());
    }

    #[test]
    fn multilingual_output_round_trips_without_duplicates() {
        let text = "中文🙂abc日本語".repeat(1000);
        for budget in [4, 5, 7, 4096] {
            let mut cursor = 0;
            let mut result = String::new();
            loop {
                let page = output_page(&text, cursor, budget).unwrap();
                assert!(page.text.len() <= budget);
                result.push_str(page.text);
                cursor = page.next_cursor;
                if !page.truncated {
                    break;
                }
            }
            assert_eq!(result, text);
        }
    }

    #[test]
    fn rejects_bad_cursors_and_budgets() {
        assert_eq!(output_page("中", 1, 4), Err(Error::InvalidCursor));
        assert_eq!(output_page("", usize::MAX, 4), Err(Error::InvalidCursor));
        assert_eq!(output_page("a", 0, 0), Err(Error::InvalidBudget));
        assert_eq!(output_page("a", 0, 4097), Err(Error::InvalidBudget));
        assert_eq!(output_page("", 0, 4).unwrap().next_cursor, 0);
    }

    #[test]
    fn sleep_requires_idle_and_respects_disable() {
        assert!(!should_sleep(7199, Some(DEFAULT_IDLE_SECONDS), false));
        assert!(should_sleep(7200, Some(DEFAULT_IDLE_SECONDS), false));
        assert!(!should_sleep(7200, Some(DEFAULT_IDLE_SECONDS), true));
        assert!(!should_sleep(u64::MAX, None, false));
    }
}

use sessanchor::{check_command_policy, Error, Execution, Session};
use std::collections::BTreeMap;

#[test]
fn sudo_prefix_requires_approval() {
    for command in [
        "sudo",
        "sudo id",
        "  sudo id",
        "\nsudo\tid",
        "sudo\nid",
        "sudo;id",
        "sudo|cat",
        "sudo&&id",
        "sudo>/tmp/out",
    ] {
        assert_eq!(
            check_command_policy(command),
            Err(Error::ApprovalRequired),
            "{command:?}"
        );
    }
}

#[test]
fn similarly_named_commands_are_not_sudo() {
    for command in ["sudoers", "sudo-check", "echo sudo", "id"] {
        assert_eq!(check_command_policy(command), Ok(()));
    }
}

#[test]
fn rejection_does_not_register_or_start_a_task() {
    let mut session = Session::default();
    let execution = |command: &str| Execution {
        command: command.into(),
        cwd: None,
        env: BTreeMap::new(),
    };
    assert_eq!(
        session.submit("r", execution("sudo id")),
        Err(Error::ApprovalRequired)
    );
    assert!(!session.busy());
    assert_eq!(session.submit("safe", execution("id")).unwrap().id, 1);
}

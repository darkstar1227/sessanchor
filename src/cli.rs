use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use sessanchor::{
    connections::{ConnectionState, ConnectionStore},
    ssh::{self, Target},
    state,
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "sanc",
    version,
    about = "Agent-friendly SSH tools (early development)"
)]
struct Cli {
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Daemon {
        #[command(subcommand)]
        command: Daemons,
    },
    /// PreToolUse adapter shared by Codex and Claude Code; reads one JSON event.
    Hook,
    /// JSON-RPC stdio MCP server; remote execution requires explicit opt-in.
    Mcp {
        #[arg(long)]
        allow_exec: bool,
    },
    Capabilities,
    /// One-shot probe, ignoring SSH config. Requires an already trusted host key.
    Probe {
        host: String,
        user: String,
        port: u16,
        identity: PathBuf,
    },
    /// Independent device configuration and cached observations.
    Device {
        #[command(subcommand)]
        command: Devices,
    },
    Session {
        #[command(subcommand)]
        command: Sessions,
    },
    /// Execute once in a session. No remote persistence on SSH loss yet.
    Exec {
        session: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        command: String,
    },
    Task {
        id: i64,
    },
    Output {
        id: i64,
        #[arg(long, default_value = "stdout")]
        stream: String,
        #[arg(long, default_value_t = 0)]
        cursor: u64,
    },
    #[command(hide = true)]
    Worker {
        id: i64,
    },
}

#[derive(Subcommand)]
enum Sessions {
    Create {
        id: String,
        #[arg(long)]
        device: String,
    },
    List,
    Describe {
        id: String,
        description: String,
    },
}

#[derive(Subcommand)]
enum Daemons {
    Run {
        #[arg(long, default_value_t = 7200)]
        idle_seconds: u64,
        #[arg(long, default_value_t = 60)]
        probe_seconds: u64,
    },
    Status,
    Stop,
    Connect {
        id: String,
    },
}

#[derive(Subcommand)]
enum Devices {
    Add {
        id: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        user: String,
        #[arg(long, default_value_t = 22)]
        port: u16,
        #[arg(long)]
        identity: PathBuf,
    },
    List,
    Probe {
        id: String,
    },
    Events {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
    },
}

fn run(cli: Cli) -> Result<Value, &'static str> {
    if let Commands::Daemon { command } = cli.command {
        let dir = state::directory(cli.state_dir).map_err(|_| "unsafe_state_directory")?;
        return match command {
            Daemons::Run {
                idle_seconds,
                probe_seconds,
            } => {
                sessanchor::daemon::serve(&dir, idle_seconds, probe_seconds)?;
                Ok(json!({"daemon":"stopped"}))
            }
            Daemons::Status => sessanchor::daemon::request(&dir, json!({"op":"status"})),
            Daemons::Stop => sessanchor::daemon::request(&dir, json!({"op":"stop"})),
            Daemons::Connect { id } => {
                sessanchor::daemon::request(&dir, json!({"op":"connect","id":id}))
            }
        };
    }
    if matches!(
        cli.command,
        Commands::Session { .. }
            | Commands::Exec { .. }
            | Commands::Task { .. }
            | Commands::Output { .. }
            | Commands::Worker { .. }
    ) {
        if let Commands::Exec { ref command, .. } = cli.command {
            sessanchor::check_command_policy(command).map_err(|_| "approval_required")?;
        }
        let dir = state::directory(cli.state_dir).map_err(|_| "unsafe_state_directory")?;
        let mut db = sessanchor::worker::store(&dir)?;
        return match cli.command {
            Commands::Session { command } => match command {
                Sessions::Create { id, device } => {
                    db.create_session(&id, &device)?;
                    Ok(json!({"session_id":id}))
                }
                Sessions::List => {
                    Ok(json!({"sessions":db.sessions().map_err(|_| "database_read_failed")?}))
                }
                Sessions::Describe { id, description } => {
                    db.describe_session(
                        &id,
                        &description,
                        state::now().map_err(|_| "clock_unavailable")?,
                    )?;
                    Ok(json!({"session_id":id}))
                }
            },
            Commands::Exec {
                session,
                request_id,
                command,
            } => {
                let task = db.reserve_task(
                    &session,
                    &request_id,
                    &command,
                    state::now().map_err(|_| "clock_unavailable")?,
                )?;
                if task.state == "accepted" {
                    sessanchor::worker::launch(&dir, task.task_id)?;
                }
                Ok(json!({"task":task,"remote_persistence":false}))
            }
            Commands::Task { id } => Ok(json!({"task":db.task(id)?})),
            Commands::Output { id, stream, cursor } => {
                sessanchor::worker::output(&dir, id, &stream, cursor)
            }
            Commands::Worker { id } => {
                drop(db);
                sessanchor::worker::execute(&dir, id)?;
                Ok(json!({"worker":"finished"}))
            }
            _ => unreachable!(),
        };
    }
    match cli.command {
        Commands::Capabilities => Ok(
            json!({"schema_version":1,"stage":"task_prototype","ssh":false,"ssh_probe":true,"tracked_exec":cfg!(unix),"devices":cfg!(unix),"daemon":false,"mcp":true,"hooks":true,"transfer":false,"durable_tasks":false}),
        ),
        Commands::Probe {
            host,
            user,
            port,
            identity,
        } => {
            ssh::probe(&Target {
                host,
                user,
                port,
                identity,
            })?;
            Ok(json!({"status":"available","persistent":false}))
        }
        Commands::Device { command } => {
            let dir = state::directory(cli.state_dir).map_err(|_| "unsafe_state_directory")?;
            let path = state::database_path(&dir).map_err(|_| "unsafe_database_file")?;
            let boot = state::boot_id().map_err(|_| "boot_identity_unavailable")?;
            let mut store =
                ConnectionStore::open(path, &boot).map_err(|_| "database_open_failed")?;
            match command {
                Devices::Add {
                    id,
                    host,
                    user,
                    port,
                    identity,
                } => {
                    store
                        .configure(
                            &id,
                            &Target {
                                host,
                                user,
                                port,
                                identity,
                            },
                        )
                        .map_err(|_| "device_configuration_rejected")?;
                    Ok(json!({"device_id":id,"status":"configured"}))
                }
                Devices::List => {
                    Ok(json!({"devices":store.devices().map_err(|_| "database_read_failed")?}))
                }
                Devices::Events { id, after } => Ok(
                    json!({"events":store.events(&id,after).map_err(|_| "database_read_failed")?}),
                ),
                Devices::Probe { id } => {
                    let target = store.target(&id).map_err(|_| "device_not_configured")?;
                    let result = ssh::probe(&target);
                    let observed = if result.is_ok() {
                        ConnectionState::Available
                    } else {
                        ConnectionState::Unknown
                    };
                    store
                        .observe(
                            &id,
                            observed,
                            state::now().map_err(|_| "clock_unavailable")?,
                            result.as_ref().err().copied(),
                        )
                        .map_err(|_| "observation_save_failed")?;
                    result?;
                    Ok(json!({"device_id":id,"status":observed,"persistent":false}))
                }
            }
        }
        _ => unreachable!(),
    }
}

pub(crate) fn invoke(args: Vec<String>, state_dir: Option<PathBuf>) -> Result<Value, &'static str> {
    let mut cli = Cli::try_parse_from(std::iter::once("sanc".into()).chain(args))
        .map_err(|_| "invalid_tool_arguments")?;
    cli.state_dir = state_dir;
    run(cli)
}

pub fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = err.print();
                return ExitCode::SUCCESS;
            }
            eprintln!(
                "{}",
                json!({"error":"unsupported_command","next_action":"Run sanc --help to check command syntax."})
            );
            return ExitCode::from(2);
        }
    };
    if matches!(cli.command, Commands::Hook) {
        return if crate::hook::serve().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(2)
        };
    }
    if let Commands::Mcp { allow_exec } = cli.command {
        return if crate::mcp::serve(cli.state_dir, allow_exec).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }
    match run(cli) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(code) => {
            eprintln!(
                "{}",
                json!({"error":code,"status":"unknown","next_action":if code=="approval_required" {"STOP. Ask the user; do not retry or rewrite the sudo command."} else {"Inspect state and configuration. Do not replay unknown tasks or bypass host verification."}})
            );
            ExitCode::from(1)
        }
    }
}

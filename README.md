# SessAnchor

Persistent SSH sessions and recoverable remote tasks for coding agents.

## Project direction

- Implement the core in Rust and distribute prebuilt binaries through npm.
- Use `sessanchor` as the intended npm package name.
- Provide `sanc` as the short command and `sessanchor` as the full command.
- Expose shared session operations through CLI and MCP interfaces.
- Allow remote tasks to continue when an agent exits or the SSH connection drops.
- Allow another agent to reconnect, access the session, and retrieve task output.
- Accept explicit connection parameters and independent configuration; do not
  require the local `~/.ssh/config` file. SSH config import is optional.

## Status

Early development preview, **not the complete specification**. Implemented:
independent device configuration, SQLite task intent/deduplication, background
OpenSSH workers, bounded output, a local monitoring daemon, stdio MCP, and minimal
Codex/Claude hook/skill assets. The leading-sudo rule is enforced in the core.

The initial backend requires installed OpenSSH and an already trusted host key.
It ignores SSH config. Remote disconnect persistence, persistent SSH connection
reuse, interactive shell sessions, human takeover UI, file transfers, password
authentication, multi-hop and complete Windows support remain unfinished.

Local npm tarball installation is supported for the machine it was built on;
no registry package has been published. See [usage](docs/usage.md) and
[agent integration](docs/integrations.md). State uses rusqlite + bundled SQLite,
not Turso. A successful local test is not a cross-platform production guarantee.

## Development

```sh
cargo run --bin sanc -- capabilities
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Use `sanc --help` to discover implemented commands. `capabilities` distinguishes
preview features from the unimplemented full SSH contract. No automatic helper
installation, host-key acceptance, or approval bypass is provided.

See [the requirements](docs/spec.md) and [implementation status](docs/implementation.md)
for scope, validation boundaries, and next steps.

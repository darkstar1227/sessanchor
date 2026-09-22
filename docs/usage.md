# Development preview

This is not the complete product described by spec.md. Requires Node.js 20+ and
OpenSSH for runtime; local device storage currently supports macOS/Linux only.
No remote helper is installed. Remote work may stop on SSH loss.

## Pi / Linux acceptance

After explicitly configuring an authorized Linux device, run from the source checkout:

```sh
cargo build --locked
node scripts/verify-linux.cjs /absolute/private/state-directory pi5
```

The opt-in script creates local journal sessions, runs read-only remote commands,
and checks CLI/MCP submission, request deduplication, errors, bounded stdout/stderr,
Unicode/binary output and background execution. It does not install a helper or
write remote files. An intentional exit 255 remains `unknown` and blocks that test
session; do not replay it. This validates Linux as a remote target, not Linux-local
installation or persistence across SSH transport loss.

## Windows remote commands

Experimental and currently paused. A nonterminating-error exit-status fix still
needs Windows live revalidation; do not treat this mode as fully verified.

Use explicit PowerShell mode for Unicode and predictable quoting:

```sh
sanc exec windows-session --request-id query-1 --shell powershell --command 'Write-Output "中文"'
```

MCP `exec` accepts the same optional `shell: "powershell"` field. Default mode
keeps the SSH server's configured shell. Shell choice is part of request identity:
changing it while reusing a request ID returns `request_conflict`.

PowerShell mode uses UTF-16LE Base64 transport and UTF-8 console output. Encoded
payloads over 7,600 characters are rejected before dispatch. On normal completion,
the last nonzero native exit code takes precedence; otherwise a failed final
PowerShell statement returns 1. Use explicit `exit N` for scripts needing different
exit semantics. Native programs may still emit their own non-UTF-8 bytes; those
remain losslessly available as hex. PowerShell stderr may contain CLIXML.
The original command is checked for leading `sudo` before encoding.

## Local npm installation

The developer builds Rust once before packing; npm installation itself neither
compiles Rust nor downloads executables in a postinstall script.

```sh
npm run build:native
npm test
npm pack
npm install -g ./sessanchor-0.1.0.tgz
sanc capabilities
```

This tarball contains only the build machine's platform/architecture binary.
Other targets, shared registry distribution, signing/provenance and release CI
remain pending. The launcher checks platform and SHA-256 for accidental mismatch;
this is not a publisher signature. `private: true` prevents accidental publishing.

## Configure, execute and inspect

Use your own host, account, key path and already verified known_hosts entry.
No automatic host-key acceptance is available. State defaults to
`$HOME/.local/state/sessanchor`; override with `--state-dir ABSOLUTE_PATH` or
`SESSANCHOR_STATE_DIR`. Existing state directories must be private (0700).

```sh
sanc device add lab --host lab.example --user operator --identity /absolute/key
sanc device probe lab
sanc device list
sanc device inspect lab --platform posix
sanc device pin lab
sanc device events lab --after 0
sanc session create maintenance --device lab
sanc session describe maintenance 'Check OS version; no changes planned.'
sanc exec maintenance --request-id os-check-1 --command 'uname -s'
sanc task 1
sanc output 1 --stream stdout --cursor 0
```

Always reuse the same request ID for retries of the same operation. Reuse the
returned task ID and output cursor; example task ID `1` is not guaranteed.
Conflicting parameters are rejected. `unknown` is not safe to replay.
Commands beginning with the `sudo` word are rejected with `approval_required`;
stop and ask the user, never rewrite the command to bypass the check.

Output is UTF-8 when possible, otherwise hex encoded, with a byte cursor.
Each page contains at most 4096 bytes of output text. Full outputs currently
have a provisional 64 MiB per-stream cap; overflow is marked, not silently
represented as complete. Global 7-day/1-GiB retention is not implemented.

Sessions currently serialize independent command tasks; they are not interactive
shells, do not retain cwd/environment, and have no human attach/takeover UI yet.
Local workers preserve jobs after normal submitter exit, not after local reboot.
Workers heartbeat; stale work becomes unknown when state is reopened after 30s.
SSH exit 255 is conservatively unknown, even if the remote command itself chose
255. Password prompts and jump hosts are not supported by this preview adapter.

## Monitoring daemon (Unix preview)

Use `device inspect DEVICE --platform windows` for Windows OS metadata. Queries
are explicit remote operations; later `device list` reads only the saved OS name,
version and timestamp. Pinned devices sort first, followed by last successful
connection. Frequency-based ranking is not implemented yet.

Run `sanc daemon run` in a dedicated terminal/service. Another terminal can run
`sanc daemon connect lab`, `sanc daemon status`, and `sanc daemon stop`.
`--idle-seconds 7200` and `--probe-seconds 60` configure the run; idle 0 disables
sleep. This is a monitoring daemon using short-lived probes, not persistent SSH
reuse. It never submits a previous command again. Background service installation
and automatic startup are not implemented yet.

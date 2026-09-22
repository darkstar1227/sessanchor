# Development preview

This is not the complete product described by spec.md. Requires Node.js 20+ and
OpenSSH for runtime; local device storage currently supports macOS/Linux only.
No remote helper is installed. Remote work may stop on SSH loss.

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

Run `sanc daemon run` in a dedicated terminal/service. Another terminal can run
`sanc daemon connect lab`, `sanc daemon status`, and `sanc daemon stop`.
`--idle-seconds 7200` and `--probe-seconds 60` configure the run; idle 0 disables
sleep. This is a monitoring daemon using short-lived probes, not persistent SSH
reuse. It never submits a previous command again. Background service installation
and automatic startup are not implemented yet.

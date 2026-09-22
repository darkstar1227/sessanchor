# Platform validation

These are measured tests, not a declaration of full platform support.
Device addresses, usernames and key paths are intentionally excluded.

Current priority: Pi / Linux remote targets. Windows work is paused by user request.

## macOS local → Pi 5 Linux ARM64

- Strict known-host-key probe through `sanc`: passed.
- Independent explicit device configuration, without SSH config: passed.
- Background read-only task with separate process group: `Linux / aarch64`, exit 0.
- Initial worker attempt before process-group isolation: unknown, retained without replay.
- Existing environment: tmux, Python 3, sha256sum found; shpool/cargo not found on PATH.
- No helper installed, no sudo, no remote files changed by these checks.
- Repeatable acceptance: `node scripts/verify-linux.cjs STATE_DIR DEVICE_ID`.
  This is opt-in and requires a preconfigured, authorized Linux target.
- 2026-09-22: seven of seven execution cases passed against Pi 5: CLI / stdio
  MCP submission, Chinese / emoji / shell metacharacters, exit 7, separate stderr,
  multi-page UTF-8 output, lossless binary output, and a background sleep task.
  The seventh case deliberately exits 255 and verifies conservative `unknown`
  handling; this is NOT a real transport-disconnection test.
- All cases check request-ID replay and conflicting-command rejection. The
  background case checks overlapping-task rejection; unknown blocks new work.
  MCP leading-sudo rejection also passed. No remote helper was deployed.
- Local regression suite: 34 Rust tests, 2 npm tests, formatting and Clippy passed
  on macOS. Native Linux installation / daemon execution are not yet measured.
- Rebuilt the darwin-arm64 npm tarball, installed in an isolated local prefix,
  and reran the same seven cases through the installed `sanc`: 7/7 passed,
  including real stdio MCP and the sudo hard stop. This is local tarball
  installation, not a public npm release or a Linux-native npm package.

## macOS local → cf-windows

Paused: expanded validation found nonterminating PowerShell errors reported as
exit 0 (8/10 execution cases passed). A same-scope status-capture fix is in the
working tree but has NOT been revalidated on Windows. Earlier successes below
must not be interpreted as complete Windows support.

- Strict host-key probe through `sanc device probe`: passed.
- `ver` as a tracked background task: exit 0, Windows build 10.0.26200.9168.
- Non-UTF-8 command output: exact bytes retained and returned as hex (44 bytes).
- Retry with identical request ID/command: returned original task ID and exit status.
- Direct `cmd /c ver` probe failed due to command wrapping; unwrapped `ver` worked.
- PowerShell 5.1.26100.9168 available; its version query also tested through `sanc`.
- Explicit `--shell powershell` UTF-16LE encoded command: Chinese UTF-8 output
  preserved; `cmd /c exit 7` returned exit 7. Identical request retry returned
  the original task ID without dispatch. A standalone `throw` returned exit 1.
- An eight-second read-only task completed after the submitting CLI exited,
  retaining both Chinese output lines and exit 0. This was not a transport-loss test.
- No software installed, privileges elevated or remote files modified by these checks.
- `sanc device inspect --platform windows`: cached Microsoft Windows 11 Pro,
  OS version 10.0.26200 via CIM and explicit UTF-8 output. Pi 5 POSIX inspection
  cached Ubuntu 24.04 independently. Listing uses these cached values only.
- This tests Windows as a remote target only, not a local Windows daemon/npm install.

## ds-home

- Initially DNS failed while local Tailscale was stopped.
- After the user started it, local Tailscale reported `Running`; the target name
  resolved, but SSH port 2222 timed out after 8 seconds.
- Tailscale reported the target peer `Online: false`, last seen
  `2026-09-11T06:23:16.1Z`. This identifies an unreachable/offline peer, not an
  SSH authentication or host-key rejection; the exact remote cause is unknown.
- No remote command executed. The target machine/Tailscale connection must become
  reachable before its SSH tests can continue. No network configuration changed.

## Still pending

Remote disconnect recovery, persistent shells, human takeover, uploads/downloads,
resume/integrity verification, local Windows ACL/daemon support, Linux local
installation, and live Claude Code/Codex integration remain unverified.

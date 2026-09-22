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

Experimental. The nonterminating-error exit-status fix has been revalidated
live against a Windows target: `node scripts/verify-windows.cjs STATE_DIR
DEVICE_ID` (10/10 read-only execution cases). See
[platform-validation.md](platform-validation.md) for details; do not treat
this mode as fully verified beyond what is measured there.

After explicitly configuring an authorized Windows device, the same opt-in
acceptance pattern as Linux applies:

```sh
node scripts/verify-windows.cjs /absolute/private/state-directory windows-device-id
```

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

`npm run build:native` writes the current machine's binary and checksum
manifest into `npm/<platform>-<arch>/` (e.g. `npm/darwin-arm64/`). A local
`npm pack`/`npm install -g` from the source checkout only exercises that one
platform/arch. The launcher (`bin/sanc.cjs`) checks platform and SHA-256 for
accidental mismatch; this is not a publisher signature.

## npm release publishing

`sessanchor` is split into a thin root package plus one binary package per
platform/arch (`@sessanchor/darwin-arm64`, `@sessanchor/darwin-x64`,
`@sessanchor/linux-x64`, `@sessanchor/linux-arm64`, `@sessanchor/win32-x64`),
listed as `optionalDependencies` on the root package — the same pattern
`esbuild`/`swc` use. Only the package matching the installer's platform/arch
is fetched.

`.github/workflows/release.yml` runs on every published GitHub Release: a
`test` job (`cargo test`/`fmt`/`clippy`, `npm test`) gates a build+publish
matrix job that builds each platform's binary on its own native runner and
publishes that platform package, followed by a final job publishing the root
`sessanchor` package once every platform package is live. Publishing
authenticates with a classic npm Automation token stored as the `NPM_TOKEN`
repo secret (`NODE_AUTH_TOKEN` env var, consumed by `actions/setup-node`'s
generated `.npmrc`); `id-token: write` is still granted separately for npm's
provenance attestation (`--provenance`), which is independent of how the
publish itself authenticates. The version published is taken from the
release's git tag (`vX.Y.Z` → `X.Y.Z`) via `scripts/sync-version.cjs`, not
from whatever is committed in `package.json`.

Before the first release, this one-time setup is required outside this repo
(the CLI/agent running this workflow cannot do it — it needs your npm login):

1. The `@sessanchor` scope must exist as an npm Organization (create it at
   npmjs.com under your account, e.g. `dst-justin`) before any
   `@sessanchor/<platform>-<arch>` package can be published — a plain
   personal-scope package (`@dst-justin/...`) would skip this step.
2. Generate an npm **Automation** token (npmjs.com → Access Tokens →
   Generate New Token → Automation; classic token, works with the installed
   npm 10.9.3 CLI — `npm trust`/OIDC trusted publishing is not available on
   this CLI version) with publish rights on the `sessanchor` org.
3. Add it as a GitHub repo secret named `NPM_TOKEN`
   (Settings → Secrets and variables → Actions → New repository secret).

`ubuntu-24.04-arm` and macOS runners may require a paid GitHub Actions plan
depending on repo visibility.

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

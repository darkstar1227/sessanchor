# Completion checklist

This checklist tracks implementation, not promises of completed support.

- [x] Rust task/control/output contract prototype and regression tests
- [x] Literal leading-sudo rejection in the shared submission core
- [x] SQLite device observations and atomic event history
- [x] Explicit-parameter strict-host-key OpenSSH probe; tested against Pi 5
- [x] Independent device CLI: configuration, cached list, probe, events (macOS tested)
- [ ] Durable task intent, request deduplication, crash/unknown recovery
  - SQLite intent/claim/retry and worker leases implemented; full recovery audit pending.
- [ ] Private single-user daemon IPC and lifecycle
- [ ] Persistent SSH connections, boot-scoped monitoring, sleep/backoff
- [ ] Password/key/agent authentication and multi-hop support
- [ ] Explicit first-host-key approval and changed-key rejection workflow
- [ ] Persistent remote tasks, helper consent and capability matrix
- [ ] Sessions, descriptions, human read-only viewer, takeover and lease expiry
- [ ] Frequent devices and cached OS metadata
- [ ] Bounded output artifacts, retention and secret-handling policy
- [ ] CLI/MCP parity and protocol tests
  - stdio read-only tools plus opt-in exec implemented; initial protocol tests passing.
- [ ] Claude Code/Codex skills/hooks and context-mode coexistence tests
  - skill and minimal PreToolUse adapters provided; live agent coexistence pending.
- [ ] File plans, integrity checks and safe destination replacement
- [ ] Large transfers, pause/cancel and restart-safe resume
- [ ] npm local installation and prebuilt platform package validation
  - darwin-arm64 tarball installed successfully in an isolated local npm prefix;
    other platform artifacts and latest-package revalidation pending.
- [ ] macOS/Linux/Windows platform acceptance and failure injection
- [ ] Documentation and full requirement-to-test audit
- [ ] Registry publication (requires release target/authority)

No remote helper installation, registry publication, or production rollout is
implied by a passing local unit test. Pending decisions in spec.md still apply.

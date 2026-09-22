---
name: sessanchor
description: Use configured SessAnchor devices for tracked SSH commands and incremental output. Applies to remote work through sanc or SessAnchor MCP, not general local shell tasks.
---

Check `sanc capabilities` before assuming a feature exists. This preview lacks
remote disconnect persistence, interactive shells, transfers and human takeover.

- Prefer configured device/session IDs. Inspect descriptions and current tasks
  before reusing a session; descriptions are notes, not authoritative state.
- Execute with a stable `request_id`. Retry the identical request with that ID;
  keep the returned task ID. Never replay `unknown` work with a new ID.
- Read `task` then `output`, retaining `next_cursor` separately for each stream.
  Do not repeatedly pull full output. Respect UTF-8/hex encoding and truncation.
- On `approval_required`, STOP and ask the user. Never rewrite sudo to bypass
  policy. Do not accept host keys, install helpers, or elevate on their behalf.
- For context-mode, process authorized local output artifacts on demand; do not
  inject full logs first or route tool calls back and forth. It is optional.
- Explain results in the user's language; preserve machine keys and IDs.

Use `sanc --help` for CLI syntax. MCP is read-only unless the user enables
`sanc mcp --allow-exec`; tool availability is not authorization for arbitrary work.

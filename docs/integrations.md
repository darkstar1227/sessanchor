# Agent integration preview

No installer changes your global configuration. Review and merge the examples
under `integrations/`; do not replace existing MCP servers, hooks or skills.

- MCP: `sanc mcp` exposes cached devices, sessions, task status and output.
  `--allow-exec` adds session creation/descriptions and tracked remote execution.
  Enabling that flag is not approval for every command; retain agent permissions.
- Codex: merge `integrations/codex.toml` into the applicable config layer.
  Merge `integrations/hooks.json` into `.codex/hooks.json` alongside active config.
- Claude Code: merge `integrations/claude.mcp.json` into the applicable MCP config,
  and merge the hook object into `.claude/settings.json`.
- Skill: copy `integrations/skills/sessanchor` to your chosen agent skill directory.
  Keep the same concise instructions for either agent; explain results in the
  user's language without translating identifiers.

The hook only handles `mcp__sessanchor__exec`. If you rename the server, update
the matcher and adapter together. Valid/unrelated events return an empty object;
only missing request IDs and leading sudo emit a bounded denial. It never
auto-approves or rewrites input. The core applies the sudo rule even without hooks.
Shell-mediated CLI calls rely on core enforcement, not this MCP hook adapter.

## context-mode design reference

Reviewed its PreToolUse entrypoint and matcher configuration: tool-specific
routing and platform output formatting are separate concerns. SessAnchor uses
the same separation, but does not copy its self-healing/config mutation logic or
redirect calls. Its adapter ignores context-mode tools and emits no success
context, avoiding a redirect loop and repeated guide injection by construction.
This is not yet a live coexistence test of both installed products.

MCP implements newline-delimited stdio JSON-RPC with protocol 2025-11-25;
network transports, full cancellation and platform UI integration are not tested.
Hooks were checked against current documentation, not a live Claude/Codex session.
Older clients without these hook events must use skill plus core error responses.

Sources checked during implementation:

- [OpenAI hooks](https://learn.chatgpt.com/docs/hooks)
- [OpenAI MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [context-mode hook configuration](https://github.com/mksglu/context-mode/blob/main/hooks/hooks.json)
- [context-mode PreToolUse adapter](https://github.com/mksglu/context-mode/blob/main/hooks/pretooluse.mjs)
- [MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)

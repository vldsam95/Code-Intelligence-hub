# Claude Code Setup

Run the installer with `--clients claude`. The adapter idempotently adds a local stdio MCP connection while preserving existing MCP servers and settings.

Verify the setup:

```bash
codeintel doctor --client claude
```

The agent policy is derived from the canonical `AGENT_OPERATING_POLICY.md`; there is no separate template with different semantics.

The skill is opt-in per project. Without an enabled marker, Claude continues normally and does not initialize Code Intelligence. With a marker, it opens local MCP automatically and loads a Wiki only through an explicit binding. When installing as root, pass `--agent-user <USER>` to target the correct account.

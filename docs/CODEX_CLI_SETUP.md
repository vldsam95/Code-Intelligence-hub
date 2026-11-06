# Codex CLI Setup

The installer detects the supported Codex configuration, preserves unrelated entries and adds one MCP server:

```json
{
  "command": "codeintel",
  "args": ["mcp", "serve", "--stdio"]
}
```

The MCP process is a lightweight adapter. It does not build the graph; it communicates with the Worker daemon through `/run/code-intelligence/worker.sock`.

After installation, run `codeintel doctor --client codex`. In a project with `.code-intel/project.yaml`, Codex should open the project, run preflight before significant changes, and finish with review, verification and a quality gate.

At session start, the skill checks only for the marker. Without it, the skill exits immediately and never runs `init`. With an enabled marker, it calls `open_project`, loads brief context and checks the Wiki binding. After one-time onboarding, Codex needs no repeated reminder to use the platform or Wiki.

On a multi-user host, install with `sudo` and pass `--agent-user <USER>` so that MCP and the skill are configured for the account running Codex.

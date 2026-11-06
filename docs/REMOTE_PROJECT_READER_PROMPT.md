# Project Access — full Agent setup

```text
PROJECT_NAME=<PROJECT_NAME>
PROJECT_ID=<PROJECT_ID>
HUB_URL=<HUB_URL>
PROJECT_ACCESS_TOKEN=<PROJECT_ACCESS_TOKEN>

Use this single Project Access for this Project only. It may be used by an authorized Codex, Claude Code, Hermes or Generic Agent; do not create a token per Agent. At session start record client/version/time metadata when the Hub is reachable.

REMOTE HUB MCP
URL: <HUB_URL>/mcp
Authorization: Bearer <PROJECT_ACCESS_TOKEN>
Optional metadata headers: X-Agent-Client, X-Agent-Version, X-Agent-Session-ID

Useful endpoints:
- GET <HUB_URL>/api/v1/projects/<PROJECT_ID>/context-pack
- GET <HUB_URL>/api/v1/projects/<PROJECT_ID>/health
- GET <HUB_URL>/api/v1/projects/<PROJECT_ID>/quality-history
- GET <HUB_URL>/api/v1/projects/<PROJECT_ID>/findings
- POST <HUB_URL>/api/v1/projects/<PROJECT_ID>/remote-analysis
- POST <HUB_URL>/api/v1/projects/<PROJECT_ID>/agent-sessions

If you only need a presentation, architecture explanation or history, use Hub context and state freshness explicitly. Do not claim line-level current truth when the Worker is offline.

If you are editing code, Project Access alone is not enough. Work on a Host/check-out where you have real filesystem access, use Local MCP (`codeintel mcp serve --stdio`), inspect current source, run preflight, edit, review, required verification and Quality Gate. The Worker then syncs structured results to the Hub. Never attempt arbitrary shell execution through Hub MCP.

For a checkout of the SAME logical Project:
CODEINTEL_PROJECT_TOKEN='<PROJECT_ACCESS_TOKEN>' codeintel hub bind --hub <HUB_URL> --project-token "$CODEINTEL_PROJECT_TOKEN" .
unset CODEINTEL_PROJECT_TOKEN

For a NEW cloned/forked Project, do not bind this token. Run `codeintel fork-project .` and create a new Hub Project through Host Access.

Never put this token in source, Git, logs or tracked `.code-intel/` files. Store it only through the Code Intelligence local binding flow.
```

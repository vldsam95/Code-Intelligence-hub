# Remote Agent Setup

A remote agent receives a project-scoped token and connects to Hub MCP. It can read context packs, code health, history and findings. When the Worker is online, it can request structured live analysis: status, architecture, symbol metadata, callers/callees, impact, coverage, quality or a read-only preflight preview.

When the Worker is offline, the Hub returns last-known data marked `STALE`/`LAST_KNOWN`. Never present this as current.

Open **Project Access & Agent Setup** on the project page. Each project has one Project Access credential for Codex, Claude, Hermes or another authorized agent. Endpoints, MCP configuration, a complete prompt and Reveal/Copy/Rotate/Revoke/Test actions are provided together. There is no separate token per agent or session.

Remote Hub access does not edit source or provide line-level proof from central context. Code changes must be made where the agent has actual source access, through local MCP. Project Access alone is not a remote shell.

A remote agent cannot onboard a project in place of the local Worker or receive full source. Visualization is generated only on demand by an online Worker, is limited to metadata/edges and is consumed once from the Hub.

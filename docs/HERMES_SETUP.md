# Hermes Setup

Run the installer with `--clients hermes`. The adapter detects the supported configuration and adds `codeintel mcp serve --stdio` without altering existing connections.

Hermes uses the same Worker, graph, risk engine and quality gate as Codex and Claude. Agents do not receive separate daemons or indexes.

The adapter installs a non-invasive skill: without an enabled `.code-intel/project.yaml`, it performs no initialization, indexing or gating. After one-time onboarding, Hermes opens the project and its Wiki binding from the manifest without repeated prompts. On a multi-user host, pass `--agent-user <USER>` to the installer.

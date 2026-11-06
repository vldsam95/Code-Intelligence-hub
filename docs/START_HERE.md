# Start here — the basic model

Code Intelligence works locally first. The Worker, graph, review, runtime and quality gate do not depend on a Hub or token.

Project owners need only two ongoing access concepts:

```text
Host Access    = Can this host create/link new projects in the Hub?
Project Access = Can an agent access the Hub for this project?
```

A Host Install Token is a single-use credential valid for 15 minutes, used only to install a new host. There is no token per Codex, Claude, Hermes or session. Worker credentials remain internal and hidden.

## Install a host

For Hub connectivity, create an installation command with an Install Token on the Hosts page. Without a token, install in local-only mode:

```bash
curl -fsSL https://code-hub.example.com/downloads/install.sh | sh -s -- \
  --hub https://code-hub.example.com \
  --local-only \
  --clients codex,claude,hermes
```

## Enable a project

Only after an explicit request:

```bash
cd /path/to/project
codeintel init
```

When Host Access is available, the project is created automatically in the Hub and receives one Project Access credential. Otherwise, it is READY locally and marked LOCAL_ONLY/PENDING.

## Clone or move

```bash
codeintel open .
```

Source code and portable `.code-intel/` inputs are sufficient to rebuild a local graph. Secrets and graph databases stay outside Git. Use Project Access to link to the same Hub project; run `codeintel fork-project` for a new logical fork.

The single canonical prompt for installation, connection and workflow is `UNIVERSAL_AGENT_BOOTSTRAP_PROMPT.md`, also available under **Prompts & Skills**.

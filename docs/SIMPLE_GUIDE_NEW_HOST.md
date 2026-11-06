# Practical guide: install Code Intelligence on a new server or computer

## Goal

Install the Worker, MCP and skill for Codex, Claude and Hermes once per host. Installation does not automatically enable Code Intelligence for every project.

## Your steps

1. To use a Hub, open **Hosts → Install New Host** and create a Host Install Token valid for 15 minutes. Local-only setup needs no token.
2. Open **Prompts & Skills**, paste the token and copy the prepared prompt.
3. Send the prompt to the agent on the new server.
4. If `curl` is blocked, download the complete ZIP from **Downloads** and send it with the prompt.
5. After completion, verify that the Worker appears `ONLINE` in the Hub.

## Complete agent prompt

```text
Install Code Intelligence on a new host. Complete every step and report a clear result.

Hub: <HUB_URL>
Optional Host Install Token: <HOST_INSTALL_TOKEN_OR_LOCAL_ONLY>
Clients to configure: codex,claude,hermes

Rules:
1. Detect the OS and architecture. On Windows, use WSL2 with systemd; do not attempt to expose a Unix socket through a native Windows service.
2. If curl and a Host Install Token are available, run:
   curl -fsSL <HUB_URL>/downloads/install.sh | sh -s -- --hub <HUB_URL> --install-token '<HOST_INSTALL_TOKEN>' --clients codex,claude,hermes
   Without a token, run the same command with `--local-only` instead of `--install-token`; do not stop installation.
3. If a manual ZIP is supplied, verify release-manifest.sig and SHA256SUMS, then run install-offline.sh; on Windows, use install-windows-wsl.ps1.
4. Never disable TLS verification or use curl -k.
5. Preserve all existing client settings; add only Code Intelligence MCP/skills idempotently.
6. Confirm that the Worker is running, the agent account can access its Unix socket and no public inbound port is exposed.
7. Run codeintel doctor for each installed client.
8. Do not run codeintel init on any project without an explicit owner request.
9. Never print Worker credentials or secrets in logs or chat.

Report the Worker version, Worker ID, Hub reachability, configured clients, doctor results and warnings.
```

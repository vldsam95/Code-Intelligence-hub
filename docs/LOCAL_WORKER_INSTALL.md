# Install a local Worker

1. To connect a Hub, create a single-use **Host Install Token** valid for 15 minutes. Local-only installation needs no token.
2. Copy the one-line command from Downloads. Use `--install-token` with a token or `--local-only` without one. If `curl` is blocked, download `codeintel-complete-agent-kit-*.zip`, transfer it to the host and run `install-offline.sh`. On Windows, use PowerShell and WSL2.
3. The installer detects the OS and architecture, verifies the signature and SHA-256 checksum, installs the binary and systemd unit, registers with the Hub and waits for the Unix socket before running `doctor`.
4. Derived data is stored under `/var/lib/code-intelligence`. Host credentials stay local and outside Git. Project Access is stored only in `.code-intel/local/` or a local vault.

```bash
# Hub-connected
curl -fsSL https://code-hub.example.com/downloads/install.sh | sh -s -- \
  --hub https://code-hub.example.com \
  --install-token '<HOST_INSTALL_TOKEN>' \
  --clients codex,claude,hermes

# Local-only: graph, review, runtime and gates without a Hub
curl -fsSL https://code-hub.example.com/downloads/install.sh | sh -s -- \
  --hub https://code-hub.example.com \
  --local-only \
  --clients codex,claude,hermes
```

Reinstalling on an enrolled host does not create another Worker. The installer checks that the Hub matches,
preserves `worker_id` and the credential, and atomically replaces the binary. For routine upgrades, use
`codeintel-update`, which verifies the release against the Ed25519 key retained during initial installation.

A Worker host does not require Python, Memgraph, Qdrant, Ollama, Redis or GitHub.

The installer configures skills/MCP only for the selected agent account. With `sudo`, it uses `SUDO_USER`; pass `--agent-user <USER>` to select an account explicitly. It preserves unrelated MCP servers and client settings.

Installing a Worker does not enable projects automatically. An enabled marker or explicit onboarding request is required to activate the project workflow.

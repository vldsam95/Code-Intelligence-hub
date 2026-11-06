# Manual installation — Code Intelligence Complete Agent Kit

This ZIP comes from the same release that serves `downloads/install.sh`. Use it when a host/agent blocks curl or files must be transferred manually.

## Contents

- Workers for Linux amd64 and arm64.
- Hub source/deployment package for the same release, for recovery or an authorized Hub migration.
- `install.sh` with a pinned public signing key and offline mode.
- `install-offline.sh` for Linux.
- `install-windows-wsl.ps1` for Windows using WSL2 and systemd.
- `SHA256SUMS`, a signed release manifest and the public verification key.
- Agent adapters, a skill, MCP configuration, documentation and a universal prompt.

## Linux

Extract the archive, open a terminal in its directory and run as root:

```bash
./install-offline.sh --hub https://code-hub.example.com \
  --install-token '<HOST_INSTALL_TOKEN>' --clients codex,claude,hermes
```

Without a Hub/token, use `--local-only`. The Worker, MCP and skills remain fully functional locally.

When running through `sudo` for another agent account, add `--agent-user '<LINUX_USER>'`.

The installer does not trust filenames alone. It verifies the Ed25519 signature of `release-manifest.json`, the hash of `SHA256SUMS`, and then the binary matching the architecture.

## Windows

The Worker uses a Unix socket and systemd, so Windows is supported through WSL2. Extract the ZIP, open an administrator PowerShell session and run:

```powershell
.\install-windows-wsl.ps1 -Hub 'https://code-hub.example.com' `
  -InstallToken '<HOST_INSTALL_TOKEN>' -Clients 'codex,claude,hermes'
```

For local-only installation from the ZIP: `.\install-windows-wsl.ps1 -LocalOnly`.

## After installation

```bash
codeintel doctor
```

Run `codeintel init` inside a project only after its owner explicitly requests Code Intelligence. Open a migrated project containing `.code-intel/` with `codeintel open` to preserve its identity.

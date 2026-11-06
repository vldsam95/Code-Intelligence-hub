# Troubleshooting

## Worker offline

Run `codeintel doctor`; check TLS, systemd and `/etc/code-intelligence/worker.yaml`. Do not open an inbound port.

## Graph stale/invalid

Run `codeintel open <path>` or `codeintel index`. The graph database is derived data: a corrupt database is marked `INVALID` and rebuilt without changing source.

## Graph build failed / `UNIQUE constraint failed: nodes.id`

On version 1.2.1 or later, do not initialize the project again or delete `.code-intel/`. Update
the existing Worker with `sudo codeintel-update`, then run the following inside the same project:

```bash
codeintel open .
```

`open` preserves `project_id`, the Hub binding and the Wiki binding, rebuilding only the derived graph
through atomic staging. If the new build also fails, the graph is not presented as CURRENT and the gate remains
UNKNOWN/BLOCKED pending investigation of `NODE_ID_COLLISION`; do not use `init` as a workaround.

## Quality UNKNOWN

Check missing required checks, coverage, source drift, analyzer/runtime availability and revision mismatches. Never bypass UNKNOWN by changing a prompt.

## Hub unavailable

Local MCP and analysis continue working. Reports remain in the spool until acknowledged.

`codeintel status` should show `hub_registration=PENDING` and `hub_sync=OFFLINE/PENDING`. Do not run `init` again. The Worker synchronizes automatically when the Hub returns; inspect `last_successful_hub_sync` and the spool with `codeintel doctor`.

## Project missing from the Hub

Check that the manifest is enabled, the Worker is enrolled, the project exists in the local registry and there is no `RECOVERY_REQUIRED`. Repeated registration is idempotent; do not create the project manually. If the manifest is lost, use a rebind token.

## Wiki unavailable

Continue Code Intelligence work normally. A disconnected remote Wiki shows `UNAVAILABLE`; multiple local candidates show `UNRESOLVED`. Never choose a Wiki arbitrarily or turn this into a quality FAIL.

## Manual ZIP installation

Verify that the ZIP came from Downloads, extract it and run the appropriate installer. Offline installation also verifies Ed25519 and SHA-256; never bypass a signature mismatch.

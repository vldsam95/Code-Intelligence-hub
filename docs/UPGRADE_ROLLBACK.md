# Upgrade & Rollback

Worker upgrades verify the signature and checksum, stop the service, replace the binary atomically, perform migrations, run doctor and mark incompatible indexes as `REBUILD_REQUIRED`.

```bash
sudo codeintel-update
```

The updater reads the Hub URL from the Worker configuration, uses a locally pinned release key,
blocks automatic MAJOR upgrades, retains the previous binary under
`/usr/local/lib/code-intelligence/`, and restores it if the service, Unix socket or `doctor`
fails to recover. It does not enroll again or change existing credentials.

A Hub upgrade starts a maintenance marker and database backup, verifies the master key, then runs Alembic migrations and health checks. If schema rollback is unsafe, restore the application together with a matching database backup.

A MAJOR version mismatch is `INCOMPATIBLE`; silent best-effort behavior is not allowed.

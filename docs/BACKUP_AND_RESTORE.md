# Hub Backup and Restore

Back up the following separately:

1. The Hub SQLite database using the online backup API.
2. Hub master encryption key.
3. Configuration and release artifacts that cannot be regenerated.

Default retention: 14 daily, 8 weekly and 6 monthly backups. Create a backup daily and immediately before migrations. Restore tests verify the database, migrations, owner login, token decryption, projects/history and Worker reconnection.

Losing the master key does not corrupt the database, but encrypted secrets become unrecoverable and credentials must be rotated.

Reference deployment commands:

```bash
scripts/backup-hub.sh
scripts/backup-hub-master-key.sh
```

The encrypted master-key backup is written to `/var/backups/code-intelligence-hub-key`, separately from database
backups. Store an additional copy and the decryption material in secure storage outside the VPS. A different directory
on the same disk does not protect against host loss. Never commit keys or passphrases to Git.

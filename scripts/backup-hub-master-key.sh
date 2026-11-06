#!/usr/bin/env bash
set -euo pipefail

source_key="${CODEINTEL_MASTER_KEY_FILE:-/etc/code-intelligence/hub-master.key}"
pass_file="${CODEINTEL_BACKUP_KEY_FILE:-/etc/code-intelligence/backup.pass}"
backup_root="${CODEINTEL_KEY_BACKUP_DIR:-/var/backups/code-intelligence-hub-key}"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
target="$backup_root/hub-master-${timestamp}.key.enc"

[[ -s "$source_key" && -s "$pass_file" ]] || {
  echo "Hub master key or backup encryption passphrase is missing." >&2
  exit 1
}
install -d -m 0700 "$backup_root"
openssl enc -aes-256-cbc -salt -pbkdf2 -iter 200000 \
  -in "$source_key" -out "$target" -pass "file:$pass_file"
chmod 0600 "$target"
sha256sum "$target" > "$target.sha256"
chmod 0600 "$target.sha256"
echo "$target"

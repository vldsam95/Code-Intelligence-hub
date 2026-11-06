#!/usr/bin/env bash
set -euo pipefail

backup_root="${CODEINTEL_BACKUP_DIR:-/var/backups/code-intelligence-hub}"
pass_file="${CODEINTEL_BACKUP_KEY_FILE:-/etc/code-intelligence/backup.pass}"
container="${CODEINTEL_HUB_CONTAINER:-codeintel-hub-api}"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
plain="/var/lib/code-intelligence-hub/backups/hub-${timestamp}.db"
encrypted="$backup_root/hub-${timestamp}.db.enc"

[[ -f "$pass_file" ]] || { echo "Backup encryption key file is missing." >&2; exit 1; }
install -d -m 0700 "$backup_root" /var/lib/code-intelligence-hub/backups
docker exec -i -u 10001:999 "$container" python - "$plain" <<'PY'
import sqlite3
import sys

source = sqlite3.connect("/data/codeintel-hub.db")
target = sqlite3.connect(sys.argv[1].replace("/var/lib/code-intelligence-hub", "/data"))
with target:
    source.backup(target)
target.close()
source.close()
PY
openssl enc -aes-256-cbc -salt -pbkdf2 -iter 200000 \
  -in "$plain" -out "$encrypted" -pass "file:$pass_file"
rm -f "$plain"
chmod 0600 "$encrypted"
checksum="$(sha256sum "$encrypted" | awk '{print $1}')"
docker exec -i -u 10001:999 -e CODEINTEL_BACKUP_PATH="$encrypted" -e CODEINTEL_BACKUP_SHA256="$checksum" \
  "$container" python - <<'PY'
import os

from hub.app.db import SessionLocal
from hub.app.models import BackupRecord, utcnow

with SessionLocal() as db:
    db.add(BackupRecord(
        kind="DAILY",
        status="SUCCEEDED",
        path=os.environ["CODEINTEL_BACKUP_PATH"],
        encrypted=True,
        checksum=os.environ["CODEINTEL_BACKUP_SHA256"],
        verified_at=utcnow(),
    ))
    db.commit()
PY
find "$backup_root" -type f -name 'hub-*.db.enc' -mtime +180 -delete
echo "$encrypted"

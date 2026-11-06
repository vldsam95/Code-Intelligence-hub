#!/usr/bin/env bash
set -euo pipefail

backup="${1:-}"
pass_file="${CODEINTEL_BACKUP_KEY_FILE:-/etc/code-intelligence/backup.pass}"
compose_file="${CODEINTEL_COMPOSE_FILE:-/opt/code-intelligence/deploy/docker-compose/docker-compose.yml}"
[[ -f "$backup" && -f "$pass_file" ]] || { echo "Usage: restore-hub-backup.sh <encrypted-backup>" >&2; exit 2; }

restore_tmp="$(mktemp)"
trap 'rm -f "$restore_tmp"' EXIT
openssl enc -d -aes-256-cbc -pbkdf2 -iter 200000 \
  -in "$backup" -out "$restore_tmp" -pass "file:$pass_file"
python3 - "$restore_tmp" <<'PY'
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
assert connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
connection.close()
PY
docker compose -f "$compose_file" down
install -m 0600 "$restore_tmp" /var/lib/code-intelligence-hub/codeintel-hub.db
chown 10001:10001 /var/lib/code-intelligence-hub/codeintel-hub.db
docker compose -f "$compose_file" up -d
for _attempt in $(seq 1 30); do
  if docker exec codeintel-hub-api python -c \
    "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/health/ready', timeout=2)" \
    >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
checksum="$(sha256sum "$backup" | awk '{print $1}')"
docker exec -i -e CODEINTEL_RESTORE_PATH="$backup" -e CODEINTEL_RESTORE_SHA256="$checksum" \
  codeintel-hub-api python - <<'PY'
import os

from hub.app.db import SessionLocal
from hub.app.models import BackupRecord, utcnow

with SessionLocal() as db:
    db.add(BackupRecord(
        kind="RESTORE_TEST",
        status="SUCCEEDED",
        path=os.environ["CODEINTEL_RESTORE_PATH"],
        encrypted=True,
        checksum=os.environ["CODEINTEL_RESTORE_SHA256"],
        verified_at=utcnow(),
    ))
    db.commit()
PY

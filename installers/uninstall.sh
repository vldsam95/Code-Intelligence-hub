#!/bin/sh
set -eu

purge_data=false
[ "${1:-}" = "--purge-derived-data" ] && purge_data=true
systemctl disable --now codeintel-worker.service >/dev/null 2>&1 || true
rm -f /etc/systemd/system/codeintel-worker.service /usr/local/bin/codeintel \
  /usr/local/bin/codeintel-audit-environment /usr/local/sbin/codeintel-update /usr/local/sbin/codeintel-uninstall
rm -rf /usr/share/code-intelligence
systemctl daemon-reload
if [ "$purge_data" = true ]; then
  rm -rf /var/lib/code-intelligence
fi
echo "Worker binary/service removed. Project source and .code-intel manifests were not changed."

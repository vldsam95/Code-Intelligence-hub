#!/bin/sh
set -eu

hub=""
enroll_token=""
enroll_token_env=""
local_only=false
clients="codex,claude,hermes"
artifact_dir=""
agent_user=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --hub) hub="${2:-}"; shift 2 ;;
    --enroll-token|--install-token) enroll_token="${2:-}"; shift 2 ;;
    --enroll-token-env) enroll_token_env="${2:-}"; shift 2 ;;
    --clients) clients="${2:-}"; shift 2 ;;
    --artifact-dir) artifact_dir="${2:-}"; shift 2 ;;
    --agent-user) agent_user="${2:-}"; shift 2 ;;
    --local-only) local_only=true; shift ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [ -n "$enroll_token_env" ]; then
  case "$enroll_token_env" in
    *[!A-Za-z0-9_]*|'') echo "Invalid enrollment-token environment variable name." >&2; exit 2 ;;
  esac
  eval "enroll_token=\${$enroll_token_env:-}"
fi

if [ "$(id -u)" -ne 0 ]; then
  echo "Run the Worker installer as root." >&2
  exit 1
fi
if [ -z "$agent_user" ]; then
  agent_user="${SUDO_USER:-root}"
fi
case "$agent_user" in *[!A-Za-z0-9._-]*|'') echo "Invalid Agent user." >&2; exit 2 ;; esac
agent_home="$(getent passwd "$agent_user" | cut -d: -f6)"
[ -n "$agent_home" ] || { echo "Agent user does not exist: $agent_user" >&2; exit 2; }
if [ -z "$hub" ] && [ -z "$artifact_dir" ]; then
  echo "Usage: install.sh --hub https://hub [--install-token TOKEN | --local-only] [--clients codex,claude,hermes]" >&2
  exit 2
fi
if [ -n "$hub" ]; then
  case "$hub" in https://*) ;; *) echo "Production Hub URL must use HTTPS." >&2; exit 2 ;; esac
  hub="${hub%/}"
fi

worker_config=/etc/code-intelligence/worker.yaml
worker_credential=/etc/code-intelligence/worker.credential
if [ -f "$worker_config" ]; then
  configured_hub="$(sed -n 's/^hub_url:[[:space:]]*//p' "$worker_config" | head -1)"
  if [ -n "$configured_hub" ] && [ "$configured_hub" != "null" ] && [ "${configured_hub%/}" != "$hub" ]; then
    echo "Existing Worker is enrolled with a different Hub; refusing an implicit rebind." >&2
    exit 1
  fi
fi
already_enrolled=false
if [ -s "$worker_config" ] && [ -s "$worker_credential" ]; then
  already_enrolled=true
fi
if [ "$already_enrolled" = false ] && [ -z "$enroll_token" ]; then
  local_only=true
fi

case "$(uname -m)" in
  x86_64|amd64) platform="linux-amd64" ;;
  aarch64|arm64) platform="linux-arm64" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

required_commands="getent groupadd usermod openssl sha256sum tar"
if [ "$agent_user" != "$(id -un)" ]; then
  required_commands="runuser $required_commands"
fi
if [ -z "$artifact_dir" ]; then
  required_commands="curl $required_commands"
fi
for required in $required_commands; do
  command -v "$required" >/dev/null 2>&1 || { echo "Missing required command: $required" >&2; exit 1; }
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

if [ -n "$artifact_dir" ]; then
  artifact_dir="$(cd "$artifact_dir" && pwd)"
  cp "$artifact_dir/release-manifest.json" "$tmp_dir/release-manifest.json"
  cp "$artifact_dir/release-manifest.sig" "$tmp_dir/release-manifest.sig"
else
  curl -fsSL "$hub/downloads/release-manifest.json" -o "$tmp_dir/release-manifest.json"
  curl -fsSL "$hub/downloads/release-manifest.sig" -o "$tmp_dir/release-manifest.sig"
fi
cat > "$tmp_dir/release-public.pem" <<'PUBLIC_KEY'
__CODEINTEL_RELEASE_PUBLIC_KEY__
PUBLIC_KEY

openssl pkeyutl -verify -pubin -inkey "$tmp_dir/release-public.pem" \
  -rawin -in "$tmp_dir/release-manifest.json" \
  -sigfile "$tmp_dir/release-manifest.sig" >/dev/null 2>&1 || {
  echo "Release manifest signature verification failed." >&2
  exit 1
}

version="$(sed -n 's/^[[:space:]]*"product_version":[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp_dir/release-manifest.json" | head -1)"
[ -n "$version" ] || { echo "Release manifest has no product version." >&2; exit 1; }
artifact="codeintel-worker-v${version}-${platform}.tar.gz"
expected="$(awk '
  /"name"[[:space:]]*:[[:space:]]*"SHA256SUMS"/ { wanted=1; next }
  wanted && /"sha256"[[:space:]]*:/ {
    line=$0
    sub(/^.*"sha256"[[:space:]]*:[[:space:]]*"/, "", line)
    sub(/".*$/, "", line)
    print line
    exit
  }
' "$tmp_dir/release-manifest.json")"
[ -n "$expected" ] || { echo "Signed checksum metadata missing." >&2; exit 1; }

if [ -n "$artifact_dir" ]; then
  cp "$artifact_dir/SHA256SUMS" "$tmp_dir/SHA256SUMS"
else
  curl -fsSL "$hub/downloads/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"
fi
printf '%s  %s\n' "$expected" "$tmp_dir/SHA256SUMS" | sha256sum -c - >/dev/null
if [ -n "$artifact_dir" ]; then
  cp "$artifact_dir/$artifact" "$tmp_dir/$artifact"
else
  curl -fsSL "$hub/downloads/$artifact" -o "$tmp_dir/$artifact"
fi
(cd "$tmp_dir" && grep "  $artifact\$" SHA256SUMS | sha256sum -c - >/dev/null)
tar -xzf "$tmp_dir/$artifact" -C "$tmp_dir"

if ! getent group codeintel >/dev/null 2>&1; then
  groupadd --system codeintel
fi
if [ "$agent_user" != "root" ]; then
  usermod -a -G codeintel "$agent_user"
fi
install -d -o root -g codeintel -m 0750 /etc/code-intelligence
install -d -o root -g codeintel -m 0770 /var/lib/code-intelligence
chgrp -R codeintel /var/lib/code-intelligence
chmod -R g+rX /var/lib/code-intelligence
install -d -m 0755 /usr/share/code-intelligence
install -d -m 0755 /usr/local/lib/code-intelligence

had_binary=false
was_active=false
if [ -x /usr/local/bin/codeintel ]; then
  had_binary=true
  cp -p /usr/local/bin/codeintel "$tmp_dir/codeintel.previous"
  old_version="$(/usr/local/bin/codeintel --version 2>/dev/null | awk '{print $2}' | tr -cd 'A-Za-z0-9._-')"
  if [ -n "$old_version" ]; then
    install -m 0755 /usr/local/bin/codeintel "/usr/local/lib/code-intelligence/codeintel-$old_version"
  fi
fi
if systemctl is-active --quiet codeintel-worker.service 2>/dev/null; then
  was_active=true
  systemctl stop codeintel-worker.service
fi

install -m 0755 "$tmp_dir/codeintel" /usr/local/bin/codeintel.next
mv /usr/local/bin/codeintel.next /usr/local/bin/codeintel

rollback_binary() {
  systemctl stop codeintel-worker.service >/dev/null 2>&1 || true
  if [ "$had_binary" = true ]; then
    install -m 0755 "$tmp_dir/codeintel.previous" /usr/local/bin/codeintel
    if [ "$was_active" = true ]; then
      systemctl start codeintel-worker.service >/dev/null 2>&1 || true
    fi
  else
    rm -f /usr/local/bin/codeintel
  fi
}

install -m 0644 "$tmp_dir/codeintel-worker.service" /etc/systemd/system/codeintel-worker.service || {
  rollback_binary; exit 1;
}
install -m 0644 "$tmp_dir/SKILL.md" /usr/share/code-intelligence/SKILL.md || {
  rollback_binary; exit 1;
}
install -d -m 0755 /usr/share/code-intelligence/references /usr/share/code-intelligence/security /usr/share/code-intelligence/templates
install -m 0644 "$tmp_dir/references/security-scenarios.md" /usr/share/code-intelligence/references/security-scenarios.md || {
  rollback_binary; exit 1;
}
install -m 0644 "$tmp_dir/security/environment-baseline.json" /usr/share/code-intelligence/security/environment-baseline.json || {
  rollback_binary; exit 1;
}
install -m 0644 "$tmp_dir/templates/security-scenarios.policy.yaml" /usr/share/code-intelligence/templates/security-scenarios.policy.yaml || {
  rollback_binary; exit 1;
}
install -m 0755 "$tmp_dir/audit-environment.py" /usr/local/bin/codeintel-audit-environment || {
  rollback_binary; exit 1;
}
install -m 0644 "$tmp_dir/release-public.pem" /usr/share/code-intelligence/release-public.pem || {
  rollback_binary; exit 1;
}
install -m 0755 "$tmp_dir/install-agent-adapters.sh" /usr/share/code-intelligence/install-agent-adapters.sh || {
  rollback_binary; exit 1;
}
install -m 0755 "$tmp_dir/update.sh" /usr/local/sbin/codeintel-update || {
  rollback_binary; exit 1;
}
install -m 0755 "$tmp_dir/uninstall.sh" /usr/local/sbin/codeintel-uninstall || {
  rollback_binary; exit 1;
}

/usr/share/code-intelligence/install-agent-adapters.sh "$clients" "$agent_user" || {
  rollback_binary; exit 1;
}
printf '%s\n' "$agent_user" > /etc/code-intelligence/agent.user
chmod 0644 /etc/code-intelligence/agent.user
if [ "$already_enrolled" = false ] && [ "$local_only" = false ]; then
  CODEINTEL_ENROLL_TOKEN="$enroll_token" /usr/local/bin/codeintel enroll --hub "$hub" --clients "$clients" || {
    rollback_binary; exit 1;
  }
elif [ "$already_enrolled" = false ]; then
  if [ -s "$worker_config" ]; then
    echo "Worker installation mode: LOCAL_ONLY (preserved existing local Host identity)."
  else
    if [ -r /proc/sys/kernel/random/uuid ]; then
      host_id="$(cat /proc/sys/kernel/random/uuid)"
    else
      random_id="$(openssl rand -hex 16)"
      host_id="$(printf '%s' "$random_id" | sed 's/^\(........\)\(....\)\(....\)\(....\)\(............\)$/\1-\2-\3-\4-\5/')"
    fi
    host_name="$(hostname 2>/dev/null || printf 'local-host')"
    cat > "$worker_config" <<EOF
schema_version: 1
hub_url: null
worker_id: null
host_id: $host_id
host_name: $host_name
credential_file: /etc/code-intelligence/worker.credential
host_access_credential_file: null
socket_path: /run/code-intelligence/worker.sock
long_poll_sec: 25
EOF
    chmod 0600 "$worker_config"
    echo "Worker installation mode: LOCAL_ONLY (Hub can be connected later)."
  fi
else
  echo "Worker enrollment: preserved existing host identity and credential"
fi
if [ -f /var/lib/code-intelligence/credentials/host-access ]; then
  chmod 0600 /var/lib/code-intelligence/credentials/host-access
fi
unset enroll_token CODEINTEL_ENROLL_TOKEN

systemctl daemon-reload || { rollback_binary; exit 1; }
systemctl enable --now codeintel-worker.service || { rollback_binary; exit 1; }
worker_ready=false
attempt=0
while [ "$attempt" -lt 30 ]; do
  if [ -S /run/code-intelligence/worker.sock ]; then
    worker_ready=true
    break
  fi
  if ! systemctl is-active --quiet codeintel-worker.service; then
    break
  fi
  attempt=$((attempt + 1))
  sleep 1
done
if [ "$worker_ready" != true ]; then
  echo "Worker did not create its local socket before the readiness deadline." >&2
  rollback_binary
  exit 1
fi
if [ "$agent_user" != "root" ]; then
  if command -v setfacl >/dev/null 2>&1; then
    setfacl -m "u:$agent_user:rwx" /run/code-intelligence
    setfacl -m "u:$agent_user:rw" /run/code-intelligence/worker.sock
  else
    echo "Agent user was added to group codeintel; start a new login shell before using MCP." >&2
  fi
fi
CODEINTEL_AGENT_HOME="$agent_home" /usr/local/bin/codeintel doctor || { rollback_binary; exit 1; }
if [ "$local_only" = true ] && [ "$already_enrolled" = false ]; then
  echo "Code Intelligence Worker $version installed successfully in LOCAL_ONLY mode."
else
  echo "Code Intelligence Worker $version installed and connected successfully."
fi

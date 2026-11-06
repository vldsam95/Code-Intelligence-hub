#!/bin/sh
set -eu

hub=""
clients=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --hub) hub="${2:-}"; shift 2 ;;
    --clients) clients="${2:-}"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [ "$(id -u)" -ne 0 ]; then
  echo "Run the Worker updater as root." >&2
  exit 1
fi

worker_config=/etc/code-intelligence/worker.yaml
pinned_key=/usr/share/code-intelligence/release-public.pem
if [ ! -s "$worker_config" ] || [ ! -x /usr/local/bin/codeintel ]; then
  echo "No existing Worker installation was found; use install.sh with an enrollment token." >&2
  exit 1
fi
configured_hub="$(sed -n 's/^hub_url:[[:space:]]*//p' "$worker_config" | head -1)"
if [ -z "$hub" ]; then
  hub="$configured_hub"
fi
hub="${hub%/}"
if [ -z "$hub" ] || [ "$hub" != "${configured_hub%/}" ]; then
  echo "Updater Hub must match the enrolled Worker Hub." >&2
  exit 1
fi
case "$hub" in https://*) ;; *) echo "Production Hub URL must use HTTPS." >&2; exit 2 ;; esac
if [ ! -s "$pinned_key" ]; then
  echo "Pinned release verification key is missing; reinstall through an authenticated recovery workflow." >&2
  exit 1
fi
if [ -z "$clients" ] && [ -s /etc/code-intelligence/clients.requested ]; then
  clients="$(sed -n '1p' /etc/code-intelligence/clients.requested)"
fi
clients="${clients:-codex,claude,hermes}"
agent_user="root"
if [ -s /etc/code-intelligence/agent.user ]; then
  agent_user="$(sed -n '1p' /etc/code-intelligence/agent.user)"
fi
case "$agent_user" in *[!A-Za-z0-9._-]*|'') echo "Stored Agent user is invalid." >&2; exit 1 ;; esac
agent_home="$(getent passwd "$agent_user" | cut -d: -f6)"
[ -n "$agent_home" ] || { echo "Stored Agent user does not exist." >&2; exit 1; }

for required in curl getent openssl sha256sum tar systemctl; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "Missing required command: $required" >&2
    exit 1
  }
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM
curl -fsSL "$hub/downloads/release-manifest.json" -o "$tmp_dir/release-manifest.json"
curl -fsSL "$hub/downloads/release-manifest.sig" -o "$tmp_dir/release-manifest.sig"
openssl pkeyutl -verify -pubin -inkey "$pinned_key" \
  -rawin -in "$tmp_dir/release-manifest.json" \
  -sigfile "$tmp_dir/release-manifest.sig" >/dev/null 2>&1 || {
  echo "Release manifest signature verification failed." >&2
  exit 1
}

version="$(sed -n 's/^[[:space:]]*"product_version":[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp_dir/release-manifest.json" | head -1)"
current_version="$(/usr/local/bin/codeintel --version | awk '{print $2}')"
if [ -z "$version" ] || [ -z "$current_version" ]; then
  echo "Unable to determine current or target version." >&2
  exit 1
fi
if [ "${version%%.*}" != "${current_version%%.*}" ]; then
  echo "Automatic update across product MAJOR versions is blocked; follow the documented migration procedure." >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) platform="linux-amd64" ;;
  aarch64|arm64) platform="linux-arm64" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
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
curl -fsSL "$hub/downloads/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"
printf '%s  %s\n' "$expected" "$tmp_dir/SHA256SUMS" | sha256sum -c - >/dev/null
curl -fsSL "$hub/downloads/$artifact" -o "$tmp_dir/$artifact"
(cd "$tmp_dir" && grep "  $artifact\$" SHA256SUMS | sha256sum -c - >/dev/null)
tar -xzf "$tmp_dir/$artifact" -C "$tmp_dir"

install -d -m 0755 /usr/local/lib/code-intelligence /usr/share/code-intelligence
install -m 0755 /usr/local/bin/codeintel \
  "/usr/local/lib/code-intelligence/codeintel-$current_version"
cp -p /usr/local/bin/codeintel "$tmp_dir/codeintel.previous"
was_active=false
if systemctl is-active --quiet codeintel-worker.service 2>/dev/null; then
  was_active=true
  systemctl stop codeintel-worker.service
fi
install -m 0755 "$tmp_dir/codeintel" /usr/local/bin/codeintel.next
mv /usr/local/bin/codeintel.next /usr/local/bin/codeintel

rollback() {
  systemctl stop codeintel-worker.service >/dev/null 2>&1 || true
  install -m 0755 "$tmp_dir/codeintel.previous" /usr/local/bin/codeintel
  if [ "$was_active" = true ]; then
    systemctl start codeintel-worker.service >/dev/null 2>&1 || true
  fi
}

install -m 0644 "$tmp_dir/codeintel-worker.service" /etc/systemd/system/codeintel-worker.service || {
  rollback; exit 1;
}
install -m 0644 "$tmp_dir/SKILL.md" /usr/share/code-intelligence/SKILL.md || {
  rollback; exit 1;
}
install -d -m 0755 /usr/share/code-intelligence/references /usr/share/code-intelligence/security /usr/share/code-intelligence/templates
install -m 0644 "$tmp_dir/references/security-scenarios.md" /usr/share/code-intelligence/references/security-scenarios.md || {
  rollback; exit 1;
}
install -m 0644 "$tmp_dir/security/environment-baseline.json" /usr/share/code-intelligence/security/environment-baseline.json || {
  rollback; exit 1;
}
install -m 0644 "$tmp_dir/templates/security-scenarios.policy.yaml" /usr/share/code-intelligence/templates/security-scenarios.policy.yaml || {
  rollback; exit 1;
}
install -m 0755 "$tmp_dir/audit-environment.py" /usr/local/bin/codeintel-audit-environment || {
  rollback; exit 1;
}
install -m 0755 "$tmp_dir/install-agent-adapters.sh" /usr/share/code-intelligence/install-agent-adapters.sh || {
  rollback; exit 1;
}
install -m 0755 "$tmp_dir/update.sh" /usr/local/sbin/codeintel-update || {
  rollback; exit 1;
}
install -m 0755 "$tmp_dir/uninstall.sh" /usr/local/sbin/codeintel-uninstall || {
  rollback; exit 1;
}
/usr/share/code-intelligence/install-agent-adapters.sh "$clients" "$agent_user" || {
  rollback; exit 1;
}
systemctl daemon-reload || { rollback; exit 1; }
systemctl enable --now codeintel-worker.service || { rollback; exit 1; }
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
  rollback
  echo "Worker update missed the local socket readiness deadline; previous binary restored." >&2
  exit 1
fi
if ! CODEINTEL_AGENT_HOME="$agent_home" /usr/local/bin/codeintel doctor; then
  rollback
  echo "Worker update failed doctor; previous binary restored." >&2
  exit 1
fi
echo "Code Intelligence Worker updated atomically from $current_version to $version."

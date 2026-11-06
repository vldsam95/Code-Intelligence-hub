#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --quiet --manifest-path "$repo_root/worker/Cargo.toml" -p codeintel-cli
binary="$repo_root/worker/target/debug/codeintel"
test_root="$(mktemp -d)"
worker_pid=""
hub_pid=""
cleanup() {
  if [ -n "$worker_pid" ]; then kill "$worker_pid" >/dev/null 2>&1 || true; fi
  if [ -n "$hub_pid" ]; then kill "$hub_pid" >/dev/null 2>&1 || true; fi
  rm -rf "$test_root"
}
trap cleanup EXIT
project="$test_root/legacy-site"
data_dir="$test_root/data"
config="$test_root/worker.yaml"
credential="$test_root/worker.credential"
socket="$test_root/offline-worker.sock"
port="$((20000 + RANDOM % 20000))"
mkdir -p "$project"
printf '%s\n' 'def main():' '    return "ok"' > "$project/app.py"

# Marker detection is non-invasive.
if CODEINTEL_DATA_DIR="$data_dir" CODEINTEL_WORKER_SOCKET="$socket" \
  CODEINTEL_WORKER_CONFIG="$config" "$binary" status "$project" >/dev/null 2>&1; then
  echo "Unmarked Project was unexpectedly activated." >&2
  exit 1
fi
test ! -e "$project/.code-intel"

printf '%s' 'test-worker-credential' > "$credential"
cat > "$config" <<EOF
schema_version: 1
hub_url: http://127.0.0.1:$port
worker_id: 10000000-0000-4000-8000-000000000001
host_id: 10000000-0000-4000-8000-000000000002
host_name: offline-acceptance
credential_file: $credential
socket_path: $socket
long_poll_sec: 1
EOF

common_env=(
  CODEINTEL_DATA_DIR="$data_dir"
  CODEINTEL_WORKER_SOCKET="$socket"
  CODEINTEL_WORKER_CONFIG="$config"
)
env "${common_env[@]}" "$binary" init "$project" > "$test_root/init-one.json" &
first_pid=$!
env "${common_env[@]}" "$binary" init "$project" > "$test_root/init-two.json" &
second_pid=$!
wait "$first_pid"
wait "$second_pid"

first_id="$(sed -n 's/.*"project_id": "\([^"]*\)".*/\1/p' "$test_root/init-one.json" | head -1)"
second_id="$(sed -n 's/.*"project_id": "\([^"]*\)".*/\1/p' "$test_root/init-two.json" | head -1)"
test -n "$first_id"
test "$first_id" = "$second_id"
test -f "$project/.code-intel/SKILL.md"
test -f "$project/.code-intel/context/LAST_VERIFIED.json"
test -f "$project/.code-intel/portability/GRAPH_BUILD_INFO.json"
grep -q '^local/$' "$project/.code-intel/.gitignore"
test ! -e "$project/.code-intel/graph.sqlite3"
grep -q '"hub_registration": "PENDING"' "$test_root/init-one.json"
grep -q '"local_success": true' "$test_root/init-one.json"

env "${common_env[@]}" "$binary" status "$project" > "$test_root/status.json"
grep -q '"hub_registration": "PENDING"' "$test_root/status.json"
grep -q '"hub_sync": "OFFLINE"' "$test_root/status.json"
test -s "$data_dir/registry.json"
test -d "$data_dir/projects/$first_id"

# A daemon restart does not lose pending registration. It retries without an Agent session and
# registers automatically when the Hub endpoint returns.
env "${common_env[@]}" "$binary" worker run > "$test_root/worker-first.log" 2>&1 &
worker_pid=$!
for _ in $(seq 1 30); do [ -S "$socket" ] && break; sleep .1; done
test -S "$socket"
kill "$worker_pid"
wait "$worker_pid" 2>/dev/null || true
worker_pid=""
rm -f "$socket"

env "${common_env[@]}" "$binary" worker run > "$test_root/worker-second.log" 2>&1 &
worker_pid=$!
sleep .5
python3 "$repo_root/fixtures/fake_registration_hub.py" "$port" "$test_root/registration.json" &
hub_pid=$!
for _ in $(seq 1 150); do [ -s "$test_root/registration.json" ] && break; sleep .1; done
test -s "$test_root/registration.json"
for _ in $(seq 1 100); do
  grep -q '"hub_registration": "REGISTERED"' "$data_dir/registry.json" && break
  sleep .1
done
grep -q '"hub_registration": "REGISTERED"' "$data_dir/registry.json"
for _ in $(seq 1 100); do
  env "${common_env[@]}" "$binary" status "$project" > "$test_root/synced-status.json"
  grep -q '"hub_sync": "CURRENT"' "$test_root/synced-status.json" && break
  sleep .1
done
grep -q '"hub_registration": "REGISTERED"' "$test_root/synced-status.json"
grep -q '"hub_sync": "CURRENT"' "$test_root/synced-status.json"
test "$(stat -c '%a' "$project/.code-intel/local")" = "700"
test "$(stat -c '%a' "$project/.code-intel/local/project-access.token")" = "600"
test "$(cat "$project/.code-intel/local/project-access.token")" = "ci_proj_v1_acceptance"
test "$(stat -c '%a' "$data_dir/credentials/host-access")" = "600"
test "$(cat "$data_dir/credentials/host-access")" = "ci_host_v1_acceptance"
python3 - "$test_root/registration.json" "$first_id" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
assert payload["project_id"] == sys.argv[2]
assert payload["instance_id"]
assert payload["revision"]["revision_id"]
assert payload["index_status"] == "CURRENT"
assert payload["wiki"]["mode"] == "none"
assert "source" not in payload and "raw_graph" not in payload
PY

echo "Onboarding acceptance passed: opt-in, concurrent identity, offline local success, restart durability and automatic registration retry."

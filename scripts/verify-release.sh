#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifact_dir="${1:-$repo_root/release/artifacts}"
version="${2:-1.3.0}"
kit="$artifact_dir/codeintel-complete-agent-kit-v${version}.zip"

for required in openssl sha256sum unzip; do
  command -v "$required" >/dev/null || { echo "Missing verification command: $required" >&2; exit 1; }
done
test -s "$kit"
(
  cd "$artifact_dir"
  sha256sum -c SHA256SUMS
  openssl pkeyutl -verify -pubin -inkey release-public.pem -rawin \
    -in release-manifest.json -sigfile release-manifest.sig
)

bundle_dir="$(mktemp -d)"
trap 'rm -rf "$bundle_dir"' EXIT
unzip -q "$kit" -d "$bundle_dir"
(
  cd "$bundle_dir"
  openssl pkeyutl -verify -pubin -inkey release-public.pem -rawin \
    -in release-manifest.json -sigfile release-manifest.sig
  sha256sum -c SHA256SUMS
)
for required in \
  "codeintel-worker-v${version}-linux-amd64.tar.gz" \
  "codeintel-worker-v${version}-linux-arm64.tar.gz" \
  "codeintel-hub-v${version}.tar.gz" \
  CODEINTEL_AGENT_PROMPT.md install-offline.sh install-windows-wsl.ps1; do
  test -s "$bundle_dir/$required"
done

public_hash="$(sha256sum "$kit" | awk '{print $1}')"
manifest_hash="$(python3 - "$artifact_dir/release-manifest.json" "$kit" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
name = pathlib.Path(sys.argv[2]).name
print(next(row["sha256"] for row in manifest["artifacts"] if row["name"] == name))
PY
)"
test "$public_hash" = "$manifest_hash"
echo "Release v$version verified: curl artifacts and the complete manual kit share one signed build."

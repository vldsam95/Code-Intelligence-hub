#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="${CODEINTEL_RELEASE_VERSION:-1.3.0}"
signing_key="${CODEINTEL_SIGNING_KEY:-}"
output_dir="${CODEINTEL_RELEASE_OUTPUT:-$repo_root/release/artifacts}"

# Fail before building or replacing artifacts if dependency notices are stale.
python3 "$repo_root/scripts/third-party-licenses.py" --check

if [[ -z "$signing_key" || ! -f "$signing_key" ]]; then
  echo "CODEINTEL_SIGNING_KEY must point to an Ed25519 private key." >&2
  exit 1
fi
for required in cargo openssl sha256sum tar zip python3; do
  command -v "$required" >/dev/null || { echo "Missing build dependency: $required" >&2; exit 1; }
done

build_root="$(mktemp -d)"
trap 'rm -rf "$build_root"' EXIT
mkdir -p "$output_dir"
find "$output_dir" -maxdepth 1 -type f -delete

write_checksums() {
  (
    cd "$output_dir"
    files=(
      codeintel-worker-v"$version"-linux-amd64.tar.gz
      codeintel-worker-v"$version"-linux-arm64.tar.gz
      codeintel-hub-v"$version".tar.gz
      codeintel-agent-adapters-v"$version".zip
      codeintel-docs-v"$version".zip
      install.sh install-offline.sh install-windows-wsl.ps1 uninstall.sh update.sh
      MANUAL_INSTALL_README.md release-public.pem
    )
    if [[ -f "codeintel-complete-agent-kit-v${version}.zip" ]]; then
      files+=("codeintel-complete-agent-kit-v${version}.zip")
    fi
    sha256sum "${files[@]}" > SHA256SUMS
  )
}

write_manifest() {
  python3 - "$output_dir" "$version" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
version = sys.argv[2]
platform_for = {
    f"codeintel-worker-v{version}-linux-amd64.tar.gz": "linux-amd64",
    f"codeintel-worker-v{version}-linux-arm64.tar.gz": "linux-arm64",
    f"codeintel-hub-v{version}.tar.gz": "hub",
    f"codeintel-agent-adapters-v{version}.zip": "any",
    f"codeintel-docs-v{version}.zip": "any",
    f"codeintel-complete-agent-kit-v{version}.zip": "linux-windows-wsl",
    "install.sh": "any-linux",
    "install-offline.sh": "any-linux",
    "install-windows-wsl.ps1": "windows-wsl",
    "uninstall.sh": "any-linux",
    "update.sh": "any-linux",
    "MANUAL_INSTALL_README.md": "any",
    "release-public.pem": "any",
    "SHA256SUMS": "any",
}
artifacts = []
for name, platform in platform_for.items():
    path = root / name
    if not path.is_file():
        continue
    data = path.read_bytes()
    artifacts.append({
        "name": name,
        "relative_path": name,
        "platform": platform,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size_bytes": len(data),
    })
payload = {
    "product_version": version,
    "agent_policy_version": 4,
    "worker_mcp_schema_version": 1,
    "hub_mcp_schema_version": 1,
    "project_manifest_schema_version": 1,
    "artifacts": artifacts,
}
(root / "release-manifest.json").write_text(
    json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
PY
}

sign_manifest() {
  openssl pkeyutl -sign -inkey "$signing_key" -rawin \
    -in "$output_dir/release-manifest.json" -out "$output_dir/release-manifest.sig"
}

build_worker() {
  local target="$1"
  local platform="$2"
  local cc_name="$3"
  local package_root="$build_root/$platform"
  if [[ -n "$cc_name" ]]; then
    command -v "$cc_name" >/dev/null || { echo "Missing cross compiler: $cc_name" >&2; exit 1; }
  fi
  rustup target add "$target" >/dev/null
  if [[ "$target" == "aarch64-unknown-linux-gnu" ]]; then
    CC_aarch64_unknown_linux_gnu="$cc_name" CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$cc_name" \
      cargo build --locked --manifest-path "$repo_root/worker/Cargo.toml" --release --target "$target" -p codeintel-cli
  else
    cargo build --locked --manifest-path "$repo_root/worker/Cargo.toml" --release --target "$target" -p codeintel-cli
  fi
  mkdir -p "$package_root"
  install -m 0755 "$repo_root/worker/target/$target/release/codeintel" "$package_root/codeintel"
  install -m 0644 "$repo_root/deploy/systemd/codeintel-worker.service" "$package_root/codeintel-worker.service"
  install -m 0755 "$repo_root/adapters/install-agent-adapters.sh" "$package_root/install-agent-adapters.sh"
  install -m 0644 "$repo_root/adapters/code-intelligence-skill/SKILL.md" "$package_root/SKILL.md"
  mkdir -p "$package_root/references" "$package_root/security" "$package_root/templates"
  install -m 0644 "$repo_root/adapters/code-intelligence-skill/references/security-scenarios.md" "$package_root/references/security-scenarios.md"
  install -m 0644 "$repo_root/security/environment-baseline.json" "$package_root/security/environment-baseline.json"
  install -m 0644 "$repo_root/templates/security-scenarios.policy.yaml" "$package_root/templates/security-scenarios.policy.yaml"
  install -m 0755 "$repo_root/scripts/audit-environment.py" "$package_root/audit-environment.py"
  install -m 0755 "$repo_root/installers/update.sh" "$package_root/update.sh"
  install -m 0755 "$repo_root/installers/uninstall.sh" "$package_root/uninstall.sh"
  install -m 0644 "$repo_root/THIRD_PARTY_NOTICES.md" "$package_root/THIRD_PARTY_NOTICES.md"
  install -m 0644 "$repo_root/LICENSE" "$package_root/LICENSE"
  cp -a "$repo_root/licenses" "$package_root/licenses"
  tar -C "$package_root" -czf "$output_dir/codeintel-worker-v${version}-${platform}.tar.gz" .
}

build_worker x86_64-unknown-linux-gnu linux-amd64 ""
build_worker aarch64-unknown-linux-gnu linux-arm64 aarch64-linux-gnu-gcc

tar \
  --exclude='__pycache__' \
  --exclude='*/__pycache__' \
  --exclude='*.pyc' \
  --exclude='*.pyo' \
  --exclude='.pytest_cache' \
  --exclude='*/.pytest_cache' \
  --exclude='.ruff_cache' \
  --exclude='*/.ruff_cache' \
  -C "$repo_root" -czf "$output_dir/codeintel-hub-v${version}.tar.gz" \
  hub pyproject.toml uv.lock alembic.ini docs spec protocol installers scripts ADR fixtures benchmarks \
  deploy/docker-compose deploy/nginx README.md IMPLEMENTATION_STATUS.md CHANGELOG.md \
  LICENSE THIRD_PARTY_NOTICES.md licenses
adapters_bundle="$build_root/adapters-bundle"
mkdir -p "$adapters_bundle"
cp -a "$repo_root/adapters/." "$adapters_bundle/"
cp "$repo_root/LICENSE" "$repo_root/THIRD_PARTY_NOTICES.md" "$adapters_bundle/"
cp -a "$repo_root/licenses" "$adapters_bundle/licenses"
(cd "$adapters_bundle" && zip -qr "$output_dir/codeintel-agent-adapters-v${version}.zip" . \
  -x '*/__pycache__/*' '*.pyc' '*.pyo')
docs_bundle="$build_root/docs-bundle"
mkdir -p "$docs_bundle/specs"
cp -a "$repo_root/docs/." "$docs_bundle/"
cp "$repo_root/spec/"*.md "$docs_bundle/specs/"
cp "$repo_root/LICENSE" "$repo_root/THIRD_PARTY_NOTICES.md" "$docs_bundle/"
cp -a "$repo_root/licenses" "$docs_bundle/licenses"
(cd "$docs_bundle" && zip -qr "$output_dir/codeintel-docs-v${version}.zip" . \
  -x '*/__pycache__/*' '*.pyc' '*.pyo')

openssl pkey -in "$signing_key" -pubout -out "$build_root/release-public.pem" >/dev/null 2>&1
awk -v key_file="$build_root/release-public.pem" '
  $0 == "__CODEINTEL_RELEASE_PUBLIC_KEY__" {
    while ((getline line < key_file) > 0) print line
    close(key_file)
    next
  }
  { print }
' "$repo_root/installers/install.sh" > "$output_dir/install.sh"
chmod 0755 "$output_dir/install.sh"
install -m 0755 "$repo_root/installers/install-offline.sh" "$output_dir/install-offline.sh"
install -m 0644 "$repo_root/installers/install-windows-wsl.ps1" "$output_dir/install-windows-wsl.ps1"
install -m 0755 "$repo_root/installers/uninstall.sh" "$output_dir/uninstall.sh"
install -m 0755 "$repo_root/installers/update.sh" "$output_dir/update.sh"
install -m 0644 "$repo_root/installers/MANUAL_INSTALL_README.md" "$output_dir/MANUAL_INSTALL_README.md"
install -m 0644 "$build_root/release-public.pem" "$output_dir/release-public.pem"

# First sign the self-contained offline component set. The complete ZIP embeds this valid
# pre-bundle manifest; the public manifest is regenerated afterward to include the ZIP itself.
write_checksums
write_manifest
sign_manifest

manual_root="$build_root/complete-agent-kit"
mkdir -p "$manual_root"
cp \
  "$output_dir/codeintel-worker-v${version}-linux-amd64.tar.gz" \
  "$output_dir/codeintel-worker-v${version}-linux-arm64.tar.gz" \
  "$output_dir/codeintel-hub-v${version}.tar.gz" \
  "$output_dir/codeintel-agent-adapters-v${version}.zip" \
  "$output_dir/codeintel-docs-v${version}.zip" \
  "$output_dir/install.sh" "$output_dir/install-offline.sh" \
  "$output_dir/install-windows-wsl.ps1" "$output_dir/uninstall.sh" "$output_dir/update.sh" \
  "$output_dir/MANUAL_INSTALL_README.md" "$output_dir/release-public.pem" \
  "$output_dir/release-manifest.json" "$output_dir/release-manifest.sig" \
  "$output_dir/SHA256SUMS" "$manual_root/"
cp "$repo_root/docs/UNIVERSAL_AGENT_BOOTSTRAP_PROMPT.md" "$manual_root/CODEINTEL_AGENT_PROMPT.md"
cp "$repo_root/LICENSE" "$repo_root/THIRD_PARTY_NOTICES.md" "$manual_root/"
cp -a "$repo_root/licenses" "$manual_root/licenses"
(cd "$manual_root" && zip -qr "$output_dir/codeintel-complete-agent-kit-v${version}.zip" .)

write_checksums
write_manifest
sign_manifest
echo "Release v$version created in $output_dir"

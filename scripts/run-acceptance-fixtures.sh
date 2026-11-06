#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_data="$(mktemp -d)"
trap 'rm -rf "$fixture_data"' EXIT

cargo build --quiet --manifest-path "$repo_root/worker/Cargo.toml" -p codeintel-cli
binary="$repo_root/worker/target/debug/codeintel"

for fixture in "$repo_root"/fixtures/projects/*; do
  CODEINTEL_DATA_DIR="$fixture_data" "$binary" revision "$fixture" >/dev/null
  result="$(CODEINTEL_DATA_DIR="$fixture_data" "$binary" index "$fixture")"
  python3 - "$fixture" "$result" <<'PY'
import json
import pathlib
import sys

fixture = pathlib.Path(sys.argv[1]).name
result = json.loads(sys.argv[2])
assert result["state"] == "CURRENT", (fixture, result)
assert result["source_revision_id"], fixture
assert not result["volatile_files"], fixture
coverage = result["coverage"]
assert coverage["framework"] in {"COMPLETE", "PARTIAL"}, (fixture, coverage)
if fixture == "python-fastapi":
    assert "fastapi" in coverage["framework_adapters"], coverage
    assert coverage["semantic"] == "COMPLETE", coverage
elif fixture == "php-laravel":
    assert "laravel" in coverage["framework_adapters"], coverage
elif fixture == "typescript-express-dynamic":
    assert "express" in coverage["framework_adapters"], coverage
    assert coverage["dynamic_boundaries"], coverage
elif fixture == "csharp-basic":
    assert "csharp" in coverage["supported_languages"], coverage
    assert "cs" not in coverage["unsupported_languages"], coverage
    assert coverage["semantic_resolved_calls"] >= 2, coverage
PY
done

"$repo_root/scripts/run-onboarding-acceptance.sh"
echo "Acceptance fixtures indexed successfully without Git or Wiki."

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo test --manifest-path worker/Cargo.toml -p codeintel-graph \
  tests::failed_atomic_publication_restores_previous_current_file -- --exact
cargo test --manifest-path worker/Cargo.toml -p codeintel-graph \
  tests::corrupt_derived_graph_can_be_atomically_replaced -- --exact
cargo test --manifest-path worker/Cargo.toml -p codeintel-runtime \
  tests::command_timeout_terminates_and_cleans_collector -- --exact
.venv/bin/pytest -q \
  hub/tests/test_api_workflow.py::test_historical_spooled_report_registers_revision_without_changing_current

echo "Failure injection checks passed: atomic publication rollback, corrupt derived DB rebuild, runtime timeout cleanup and idempotent historical spool handling."

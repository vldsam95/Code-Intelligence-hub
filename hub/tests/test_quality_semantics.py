from pathlib import Path

from hub.app.security import TokenVault
from hub.app.services.quality import (
    GateInputs,
    absence_semantics,
    dataflow_verdict,
    evaluate_gate,
    runtime_semantics,
)


def test_binary_master_key_keeps_whitespace_valued_boundary_bytes(tmp_path: Path) -> None:
    key_path = tmp_path / "master.key"
    key_path.write_bytes(b"\n" + (b"k" * 30) + b" ")
    vault = TokenVault.from_file(key_path)
    encrypted, nonce = vault.encrypt("secret", b"record")
    assert vault.decrypt(encrypted, nonce, b"record") == "secret"


def test_stale_and_missing_evidence_never_pass() -> None:
    stale = evaluate_gate(GateInputs("a", "b"))
    missing = evaluate_gate(
        GateInputs("a", "a", required_checks=("tests", "smoke"), completed_checks=("tests",))
    )
    assert (stale.verdict, stale.currentness) == ("UNKNOWN", "UNVERIFIED_CHANGE")
    assert missing.verdict == "UNKNOWN"


def test_failures_override_clean_structural_review() -> None:
    result = evaluate_gate(
        GateInputs(
            "a",
            "a",
            required_checks=("review", "smoke"),
            completed_checks=("review", "smoke"),
            failed_checks=("smoke",),
        )
    )
    assert result.verdict == "FAIL"


def test_graph_runtime_and_dataflow_distrust_invariants() -> None:
    assert (
        absence_semantics(result_count=0, supported_scope_complete=False)["absence_proven"] is False
    )
    assert runtime_semantics(observed=False) == "NOT_OBSERVED"
    assert dataflow_verdict(path_found=False, scope_complete=False) == "UNKNOWN"
    assert dataflow_verdict(path_found=False, scope_complete=True) == "NO_FLOW"

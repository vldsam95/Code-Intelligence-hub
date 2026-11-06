from __future__ import annotations

from dataclasses import dataclass, field

VALID_VERDICTS = {"PASS", "PASS_WITH_WARNINGS", "UNKNOWN", "FAIL"}


@dataclass(frozen=True)
class GateInputs:
    source_revision_id: str
    current_source_revision_id: str | None
    required_checks: tuple[str, ...] = ()
    completed_checks: tuple[str, ...] = ()
    blocking_findings: int = 0
    blocking_unknowns: tuple[str, ...] = ()
    failed_checks: tuple[str, ...] = ()
    warnings: tuple[str, ...] = ()
    graph_state: str = "CURRENT"
    unsupported_required_areas: tuple[str, ...] = ()


@dataclass(frozen=True)
class GateResult:
    verdict: str
    currentness: str
    reasons: tuple[str, ...] = field(default_factory=tuple)
    warnings: tuple[str, ...] = field(default_factory=tuple)


def evaluate_gate(inputs: GateInputs) -> GateResult:
    """Deterministic false-confidence guard used by Hub and mirrored by Worker.

    PASS is possible only for evidence pinned to the exact current revision. Missing or
    unsupported required evidence is UNKNOWN. A concrete regression/finding is FAIL.
    """

    if not inputs.current_source_revision_id:
        return GateResult("UNKNOWN", "STALE", ("current source revision is unavailable",))
    if inputs.source_revision_id != inputs.current_source_revision_id:
        return GateResult(
            "UNKNOWN", "UNVERIFIED_CHANGE", ("evidence belongs to another source revision",)
        )
    if inputs.graph_state != "CURRENT":
        return GateResult("UNKNOWN", "CURRENT", (f"graph state is {inputs.graph_state}",))
    if inputs.failed_checks:
        return GateResult(
            "FAIL",
            "CURRENT",
            tuple(f"required check failed: {check}" for check in inputs.failed_checks),
            inputs.warnings,
        )
    if inputs.blocking_findings:
        return GateResult(
            "FAIL", "CURRENT", ("blocking review finding remains open",), inputs.warnings
        )
    missing = sorted(set(inputs.required_checks) - set(inputs.completed_checks))
    unknown_reasons = [f"required check missing: {check}" for check in missing]
    unknown_reasons.extend(inputs.blocking_unknowns)
    unknown_reasons.extend(
        f"unsupported required area: {area}" for area in inputs.unsupported_required_areas
    )
    if unknown_reasons:
        return GateResult("UNKNOWN", "CURRENT", tuple(unknown_reasons), inputs.warnings)
    if inputs.warnings:
        return GateResult("PASS_WITH_WARNINGS", "CURRENT", (), inputs.warnings)
    return GateResult("PASS", "CURRENT")


def absence_semantics(*, result_count: int, supported_scope_complete: bool) -> dict[str, object]:
    return {
        "result_count": result_count,
        "absence_proven": result_count == 0 and supported_scope_complete,
        "coverage": "COMPLETE" if supported_scope_complete else "PARTIAL",
        "warning": None
        if result_count or supported_scope_complete
        else "No graph result is not proof that the relationship does not exist.",
    }


def runtime_semantics(*, observed: bool) -> str:
    return "OBSERVED" if observed else "NOT_OBSERVED"


def dataflow_verdict(*, path_found: bool, scope_complete: bool) -> str:
    if path_found:
        return "FOUND"
    return "NO_FLOW" if scope_complete else "UNKNOWN"

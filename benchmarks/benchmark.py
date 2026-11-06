#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ARMS = {"baseline", "platform"}
QUALITY_FIELDS = {
    "task_correct": 0.60,
    "false_confidence_safe": 0.15,
    "regression_prevented": 0.10,
}


class BenchmarkError(ValueError):
    pass


@dataclass(frozen=True)
class Result:
    task_id: str
    arm: str
    task_correct: bool
    false_pass: bool
    appropriate_unknown: bool
    regressions: int
    defects_found_before_completion: int
    input_tokens: int | None
    output_tokens: int | None
    tool_calls: int
    file_reads: int
    grep_search_calls: int
    context_bytes: int
    wall_time_ms: int
    peak_rss_bytes: int | None
    disk_bytes: int | None
    finding_ids: list[str]

    @classmethod
    def from_mapping(cls, value: dict[str, Any]) -> Result:
        required = {
            "task_id",
            "arm",
            "task_correct",
            "false_pass",
            "appropriate_unknown",
            "regressions",
            "defects_found_before_completion",
            "tool_calls",
            "file_reads",
            "grep_search_calls",
            "context_bytes",
            "wall_time_ms",
        }
        missing = sorted(required - value.keys())
        if missing:
            raise BenchmarkError(f"result missing fields: {', '.join(missing)}")
        if value["arm"] not in ARMS:
            raise BenchmarkError(f"invalid arm: {value['arm']}")
        normalized = {field: value.get(field) for field in cls.__dataclass_fields__}
        normalized["finding_ids"] = value.get("finding_ids", [])
        if not isinstance(normalized["finding_ids"], list) or not all(
            isinstance(item, str) and item for item in normalized["finding_ids"]
        ):
            raise BenchmarkError("finding_ids must be a list of non-empty strings")
        return cls(**normalized)

    @property
    def total_tokens(self) -> int | None:
        if self.input_tokens is None or self.output_tokens is None:
            return None
        return self.input_tokens + self.output_tokens


def read_tasks(path: Path) -> dict[str, dict[str, Any]]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema_version") != 1:
        raise BenchmarkError("unsupported task schema_version")
    tasks = document.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        raise BenchmarkError("tasks must be a non-empty list")
    indexed: dict[str, dict[str, Any]] = {}
    for task in tasks:
        task_id = task.get("task_id")
        if not task_id or task_id in indexed:
            raise BenchmarkError(f"missing or duplicate task_id: {task_id!r}")
        for field in ("project_path", "category", "prompt", "verification_argv"):
            if not task.get(field):
                raise BenchmarkError(f"{task_id}: missing {field}")
        required_findings = task.get("required_findings", [])
        if not isinstance(required_findings, list) or not all(
            isinstance(item, str) and item for item in required_findings
        ):
            raise BenchmarkError(f"{task_id}: required_findings must be a string list")
        if len(set(required_findings)) != len(required_findings):
            raise BenchmarkError(f"{task_id}: required_findings must be unique")
        indexed[task_id] = task
    return indexed


def read_results(path: Path) -> list[Result]:
    if not path.exists():
        return []
    results = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            results.append(Result.from_mapping(json.loads(line)))
        except (json.JSONDecodeError, BenchmarkError, TypeError) as error:
            raise BenchmarkError(f"{path}:{line_number}: {error}") from error
    return results


def validate_measurement(value: dict[str, Any], task_id: str, arm: str) -> Result:
    supplied_task = value.get("task_id")
    supplied_arm = value.get("arm")
    if supplied_task not in (None, task_id):
        raise BenchmarkError("measurement task_id differs from --task")
    if supplied_arm not in (None, arm):
        raise BenchmarkError("measurement arm differs from --arm")
    value = {**value, "task_id": task_id, "arm": arm}
    for token_field in ("input_tokens", "output_tokens"):
        value.setdefault(token_field, None)
    for resource_field in ("peak_rss_bytes", "disk_bytes"):
        value.setdefault(resource_field, None)
    value.setdefault("finding_ids", [])
    result = Result.from_mapping(value)
    numeric_fields = (
        "regressions",
        "defects_found_before_completion",
        "tool_calls",
        "file_reads",
        "grep_search_calls",
        "context_bytes",
        "wall_time_ms",
    )
    if any(getattr(result, field) < 0 for field in numeric_fields):
        raise BenchmarkError("measurement counters cannot be negative")
    return result


def paired_results(tasks: dict[str, dict[str, Any]], results: list[Result]) -> dict[str, dict[str, Result]]:
    pairs: dict[str, dict[str, Result]] = defaultdict(dict)
    duplicates = Counter((result.task_id, result.arm) for result in results)
    duplicate_keys = [key for key, count in duplicates.items() if count > 1]
    if duplicate_keys:
        raise BenchmarkError(f"duplicate task/arm results: {duplicate_keys}")
    for result in results:
        if result.task_id not in tasks:
            raise BenchmarkError(f"result references unknown task: {result.task_id}")
        pairs[result.task_id][result.arm] = result
    return pairs


def percent_change(baseline: float, platform: float) -> float | None:
    if baseline == 0:
        return None
    return (platform - baseline) / baseline * 100.0


def median_optional(values: list[int | None]) -> float | None:
    known = [value for value in values if value is not None]
    return statistics.median(known) if known else None


def task_score(result: Result) -> float:
    false_confidence_safe = not result.false_pass
    regression_prevented = result.regressions == 0
    efficiency_proxy = 1.0 / (1.0 + math.log1p(result.file_reads + result.grep_search_calls))
    resource_proxy = 1.0 / (1.0 + math.log1p(max(result.context_bytes, 0)))
    latency_proxy = 1.0 / (1.0 + math.log1p(max(result.wall_time_ms, 0)))
    return (
        float(result.task_correct) * QUALITY_FIELDS["task_correct"]
        + float(false_confidence_safe) * QUALITY_FIELDS["false_confidence_safe"]
        + float(regression_prevented) * QUALITY_FIELDS["regression_prevented"]
        + efficiency_proxy * 0.07
        + resource_proxy * 0.04
        + latency_proxy * 0.04
    )


def summarize(tasks: dict[str, dict[str, Any]], results: list[Result]) -> dict[str, Any]:
    pairs = paired_results(tasks, results)
    complete = {task_id: arms for task_id, arms in pairs.items() if set(arms) == ARMS}
    baseline = [arms["baseline"] for arms in complete.values()]
    platform = [arms["platform"] for arms in complete.values()]
    baseline_tokens = median_optional([result.total_tokens for result in baseline])
    platform_tokens = median_optional([result.total_tokens for result in platform])
    baseline_reads = statistics.median(
        [result.file_reads + result.grep_search_calls for result in baseline]
    ) if baseline else None
    platform_reads = statistics.median(
        [result.file_reads + result.grep_search_calls for result in platform]
    ) if platform else None
    report = {
        "schema_version": 1,
        "dataset_tasks": len(tasks),
        "paired_tasks": len(complete),
        "production_dataset_size_met": len(tasks) >= 20,
        "all_tasks_paired": len(complete) == len(tasks),
        "baseline": arm_summary(baseline),
        "platform": arm_summary(platform),
        "security_findings": {
            "baseline": security_finding_summary(tasks, baseline),
            "platform": security_finding_summary(tasks, platform),
        },
        "median_token_change_percent": (
            percent_change(baseline_tokens, platform_tokens)
            if baseline_tokens is not None and platform_tokens is not None
            else None
        ),
        "median_raw_read_change_percent": (
            percent_change(baseline_reads, platform_reads)
            if baseline_reads is not None and platform_reads is not None
            else None
        ),
    }
    hard_targets = {
        "no_false_pass_increase": report["platform"]["false_passes"]
        <= report["baseline"]["false_passes"],
        "no_quality_regression": report["platform"]["task_correct_rate"]
        >= report["baseline"]["task_correct_rate"],
        "median_tokens_not_worse": report["median_token_change_percent"] is not None
        and report["median_token_change_percent"] <= 0,
        "dataset_complete": report["production_dataset_size_met"] and report["all_tasks_paired"],
    }
    baseline_security = report["security_findings"]["baseline"]
    platform_security = report["security_findings"]["platform"]
    hard_targets["security_recall_not_worse"] = (
        baseline_security["expected"] == 0
        or platform_security["recall"] >= baseline_security["recall"]
    )
    report["hard_targets"] = hard_targets
    report["production_benchmark_pass"] = all(hard_targets.values())
    return report


def security_finding_summary(
    tasks: dict[str, dict[str, Any]], results: list[Result]
) -> dict[str, Any]:
    expected = 0
    reported = 0
    matched = 0
    task_count = 0
    for result in results:
        required = set(tasks[result.task_id].get("required_findings", []))
        if not required:
            continue
        task_count += 1
        observed = set(result.finding_ids)
        expected += len(required)
        reported += len(observed)
        matched += len(required & observed)
    return {
        "tasks": task_count,
        "expected": expected,
        "reported": reported,
        "matched": matched,
        "recall": matched / expected if expected else None,
        "precision": matched / reported if reported else None,
    }


def arm_summary(results: list[Result]) -> dict[str, Any]:
    count = len(results)
    return {
        "tasks": count,
        "task_correct_rate": sum(result.task_correct for result in results) / count if count else 0,
        "false_passes": sum(result.false_pass for result in results),
        "appropriate_unknowns": sum(result.appropriate_unknown for result in results),
        "regressions": sum(result.regressions for result in results),
        "defects_found_before_completion": sum(
            result.defects_found_before_completion for result in results
        ),
        "median_total_tokens": median_optional([result.total_tokens for result in results]),
        "median_tool_calls": statistics.median([result.tool_calls for result in results])
        if results
        else None,
        "median_file_and_search_calls": statistics.median(
            [result.file_reads + result.grep_search_calls for result in results]
        )
        if results
        else None,
        "median_context_bytes": statistics.median([result.context_bytes for result in results])
        if results
        else None,
        "median_wall_time_ms": statistics.median([result.wall_time_ms for result in results])
        if results
        else None,
        "weighted_score": statistics.mean([task_score(result) for result in results])
        if results
        else 0,
    }


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Code Intelligence Codex A/B benchmark harness")
    commands = root.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("--tasks", type=Path, required=True)
    record = commands.add_parser("record")
    record.add_argument("--tasks", type=Path)
    record.add_argument("--results", type=Path, required=True)
    record.add_argument("--task", required=True)
    record.add_argument("--arm", choices=sorted(ARMS), required=True)
    record.add_argument("--input-json", type=Path, required=True)
    summary = commands.add_parser("summarize")
    summary.add_argument("--tasks", type=Path, required=True)
    summary.add_argument("--results", type=Path, required=True)
    summary.add_argument("--output", type=Path)
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "validate":
            tasks = read_tasks(args.tasks)
            print(json.dumps({"valid": True, "tasks": len(tasks)}, indent=2))
            return 0
        if args.command == "record":
            if args.tasks:
                tasks = read_tasks(args.tasks)
                if args.task not in tasks:
                    raise BenchmarkError(f"unknown task: {args.task}")
            measurement = json.loads(args.input_json.read_text(encoding="utf-8"))
            result = validate_measurement(measurement, args.task, args.arm)
            existing = read_results(args.results)
            if any(item.task_id == result.task_id and item.arm == result.arm for item in existing):
                raise BenchmarkError("task/arm already recorded; preserve immutable benchmark evidence")
            args.results.parent.mkdir(parents=True, exist_ok=True)
            with args.results.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(result.__dict__, sort_keys=True) + "\n")
            print(json.dumps({"recorded": True, "task": args.task, "arm": args.arm}))
            return 0
        tasks = read_tasks(args.tasks)
        report = summarize(tasks, read_results(args.results))
        encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(encoded, encoding="utf-8")
        print(encoded, end="")
        return 0 if report["production_benchmark_pass"] else 2
    except (BenchmarkError, OSError, json.JSONDecodeError) as error:
        print(json.dumps({"error": str(error)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())


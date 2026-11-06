from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("benchmark.py")
SPEC = importlib.util.spec_from_file_location("codeintel_benchmark", MODULE_PATH)
assert SPEC and SPEC.loader
benchmark = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = benchmark
SPEC.loader.exec_module(benchmark)


def measurement(task_id: str, arm: str, *, tokens: int, correct: bool = True) -> dict:
    return {
        "task_id": task_id,
        "arm": arm,
        "task_correct": correct,
        "false_pass": False,
        "appropriate_unknown": False,
        "regressions": 0,
        "defects_found_before_completion": 1,
        "input_tokens": tokens - 10,
        "output_tokens": 10,
        "tool_calls": 3,
        "file_reads": 4,
        "grep_search_calls": 2,
        "context_bytes": 1000,
        "wall_time_ms": 500,
        "peak_rss_bytes": 100,
        "disk_bytes": 100,
        "finding_ids": [],
    }


def test_summary_requires_full_production_dataset() -> None:
    tasks = {"one": {}}
    results = [
        benchmark.Result.from_mapping(measurement("one", "baseline", tokens=100)),
        benchmark.Result.from_mapping(measurement("one", "platform", tokens=70)),
    ]
    report = benchmark.summarize(tasks, results)
    assert report["median_token_change_percent"] == -30
    assert report["production_benchmark_pass"] is False
    assert report["hard_targets"]["dataset_complete"] is False


def test_missing_token_export_stays_unknown() -> None:
    value = measurement("one", "baseline", tokens=100)
    value["input_tokens"] = None
    result = benchmark.Result.from_mapping(value)
    assert result.total_tokens is None


def test_security_finding_recall_is_scored_per_arm() -> None:
    tasks = {"one": {"required_findings": ["idor", "budget_race"]}}
    baseline_value = measurement("one", "baseline", tokens=100)
    baseline_value["finding_ids"] = ["idor", "budget_race"]
    platform_value = measurement("one", "platform", tokens=90)
    platform_value["finding_ids"] = ["idor"]
    report = benchmark.summarize(
        tasks,
        [
            benchmark.Result.from_mapping(baseline_value),
            benchmark.Result.from_mapping(platform_value),
        ],
    )
    assert report["security_findings"]["baseline"]["recall"] == 1
    assert report["security_findings"]["platform"]["recall"] == 0.5
    assert report["hard_targets"]["security_recall_not_worse"] is False

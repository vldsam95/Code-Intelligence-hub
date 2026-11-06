#!/usr/bin/env python3
"""Offline, read-only environment drift audit. Never downloads or installs tools."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPOSITORY_BASELINE = ROOT / "security" / "environment-baseline.json"
INSTALLED_BASELINE = pathlib.Path("/usr/share/code-intelligence/security/environment-baseline.json")
BASELINE = REPOSITORY_BASELINE if REPOSITORY_BASELINE.is_file() else INSTALLED_BASELINE


def version_tuple(raw: str) -> tuple[int, ...]:
    match = re.search(r"(\d+)\.(\d+)(?:\.(\d+))?", raw)
    return tuple(int(part or 0) for part in match.groups()) if match else ()


def probe(command: str, arguments: list[str]) -> dict[str, object]:
    binary = shutil.which(command)
    if not binary:
        return {"status": "CRITICAL", "reason": "missing", "binary": None}
    try:
        result = subprocess.run(
            [binary, *arguments],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError) as error:
        return {"status": "CRITICAL", "reason": type(error).__name__, "binary": binary}
    output = (result.stdout or result.stderr).splitlines()
    first_line = output[0][:200] if output else ""
    return {
        "status": "PASS" if result.returncode == 0 else "CRITICAL",
        "binary": binary,
        "version_output": first_line,
        "version": version_tuple(first_line),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=pathlib.Path, default=BASELINE)
    arguments = parser.parse_args()
    baseline = json.loads(arguments.baseline.read_text(encoding="utf-8"))
    tools = {
        "python": probe(sys.executable, ["--version"]),
        "rustc": probe("rustc", ["--version"]),
        "cargo": probe("cargo", ["--version"]),
        "git": probe("git", ["--version"]),
    }
    critical: list[str] = []
    warnings: list[str] = []
    for name, required in baseline["minimum_versions"].items():
        observed = tools[name]
        required_tuple = version_tuple(required)
        observed_tuple = tuple(observed.get("version", ()))
        if observed["status"] != "PASS":
            critical.append(f"{name} is unavailable")
        elif observed_tuple < required_tuple:
            critical.append(f"{name} {observed_tuple} is below reviewed minimum {required_tuple}")
        blocked = {
            version_tuple(value) for value in baseline.get("blocked_versions", {}).get(name, [])
        }
        if observed_tuple and observed_tuple in blocked:
            critical.append(f"{name} {observed_tuple} is blocked by the reviewed baseline")
    reviewed = dt.date.fromisoformat(baseline["reviewed_at"])
    age_days = (dt.datetime.now(dt.UTC).date() - reviewed).days
    if age_days > int(baseline["maximum_review_age_days"]):
        warnings.append(
            "The bundled review baseline is stale; compare and review updates manually before replacing pins."
        )
    report = {
        "status": "CRITICAL_GAP" if critical else ("REVIEW_REQUIRED" if warnings else "PASS"),
        "network_used": False,
        "automatic_updates": False,
        "baseline": str(arguments.baseline),
        "baseline_age_days": age_days,
        "tools": tools,
        "critical_gaps": critical,
        "warnings": warnings,
        "reviewed_sources": baseline["reviewed_sources"],
        "bundled_components": baseline.get("bundled_components", []),
        "next": "Review and pin a signed release; this audit never changes the environment.",
    }
    print(json.dumps(report, indent=2, default=list))
    return 2 if critical else 0


if __name__ == "__main__":
    raise SystemExit(main())

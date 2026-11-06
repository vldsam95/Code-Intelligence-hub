# Codex A/B benchmark harness

The harness compares `Codex CLI alone` against the same Codex model and settings with Code Intelligence.
It never invents token counts: fields not supplied by the client remain `null`; proxy metrics are recorded separately.

`tasks.example.json` illustrates the schema only. It is not the required dataset of 20–30 real tasks.
Before a production evaluation, copy it to `tasks.local.json`, replace the examples with real projects and tasks,
and run each task in both arms from the same initial source revision.

Security evaluations can also use `security-tasks.example.json`. Its optional `required_findings` field defines
stable, predetermined identifiers from approved ground truth; each arm returns `finding_ids`. The summary measures recall
and precision separately and fails the benchmark if Code Intelligence reduces recall. A single example task
does not establish value: an expand/freeze decision requires 20–30 paired tasks, identical model settings, and token/tool/read
costs. The example file is not a benchmark result and makes no claim that Code Intelligence performs better.

```bash
python3 benchmarks/benchmark.py validate --tasks benchmarks/tasks.local.json
python3 benchmarks/benchmark.py record --results benchmarks/results.local.jsonl \
  --task task-001 --arm baseline --input-json /path/to/measurement.json
python3 benchmarks/benchmark.py record --results benchmarks/results.local.jsonl \
  --task task-001 --arm platform --input-json /path/to/measurement.json
python3 benchmarks/benchmark.py summarize --tasks benchmarks/tasks.local.json \
  --results benchmarks/results.local.jsonl
```

Files matching `*.local.*` and raw results are excluded from Git by default. An approved release report may be saved
under an explicit filename in `benchmarks/reports/` after review.


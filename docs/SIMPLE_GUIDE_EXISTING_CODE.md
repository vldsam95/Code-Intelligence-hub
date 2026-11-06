# Practical guide: work on an enabled codebase

## Goal

Request a normal code change while the agent automatically uses Code Intelligence and selects the required verification depth.

## Your steps

1. Open the project directory and start your usual agent.
2. Request the change. The skill detects `.code-intel/project.yaml` automatically.
3. Check that the final chat response includes a quality-gate report.

## Complete agent prompt

```text
Complete the following task in the current project: <TASK>

The project may be managed by Code Intelligence. Perform only a lightweight marker check. If enabled:
1. Open the project through local MCP and retrieve brief context and the current revision.
2. If a Wiki is bound, use it only for durable knowledge; actual source and the Worker provide current technical truth.
3. Run preflight_change before significant changes and follow required_checks.
4. Use graph/semantic results for discovery, then read the relevant actual source.
5. Make the change using your normal editing tools.
6. Refresh the revision; run review_change, verify_change and all checks required by the risk engine.
7. Run quality_gate for the exact revision. On FAIL or blocking UNKNOWN, fix and repeat where possible.
8. Never infer that callers or flows are absent from an empty graph. Runtime evidence covers only the observed workload. Dead code remains a candidate.

Finish with a chat report covering the quality gate, revision, changes, verification, tests/smoke/runtime/data-flow, warnings/unknowns and whether a Wiki update is needed.
```

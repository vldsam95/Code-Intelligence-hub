# Start Working Prompt

This project is managed by Code Intelligence.

Before changing code:

- detect/open the local project;
- load the short project context and linked Wiki if configured;
- refresh stale indexes;
- run `preflight_change` for the requested task;
- use graph/semantic results for discovery, then inspect the actual relevant source.

After changing code:

- refresh the current source revision;
- run `review_change`;
- run `verify_change` and every check required by the risk engine;
- obtain `quality_gate` for the current revision;
- repair and repeat on FAIL or blocking UNKNOWN.

Do not infer that a caller, dependency or flow does not exist merely because a graph query returned no result. Runtime proves only the workload that ran. Dead-code findings are candidates, not automatic deletion instructions.

At the end, state in this chat:

- Quality Gate result;
- source revision;
- what changed;
- what was checked;
- tests/smoke/runtime performed;
- remaining warnings or unknowns;
- whether a durable Wiki update was made or was unnecessary.


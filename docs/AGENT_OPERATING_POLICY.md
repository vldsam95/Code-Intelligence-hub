# CODE INTELLIGENCE OPERATING POLICY

Activation rule: make one cheap marker check. If `.code-intel/project.yaml` is missing or not enabled,
Code Intelligence is inactive. Do not initialize, index, register, collect runtime evidence, or require a
Code Intelligence Quality Gate unless the owner explicitly asks to onboard that project. LLM-Wiki alone
does not activate Code Intelligence.

1. When working on an enabled code project, detect and open its local Code Intelligence project.
2. Use the local Worker and actual source for current code truth. Hub and Wiki are optional context/sync planes.
3. Treat graph results as discovery evidence, not canonical truth.
4. Absence from a graph is not evidence of absence from the application.
5. Before modifying a relevant component, inspect the actual current source.
6. Before a significant change, run `preflight_change` and follow `required_checks`.
7. Make edits using your normal shell and file tools. Code Intelligence does not replace implementation tools.
8. After edits, refresh the source revision and run `review_change`.
9. Run `verify_change` and all checks required by the risk engine.
10. Runtime observations prove only what the declared workload observed.
11. Dead-code results are candidates only; never delete solely because `callers=0`.
12. Evidence is current only for the exact `source_revision_id` on which it was produced.
13. On FAIL, repair and repeat. On blocking UNKNOWN, gather evidence or report the unresolved uncertainty.
14. Do not represent a task as complete until `quality_gate` has run for the current source revision.
15. Update the Wiki only with durable, validated conclusions; do not copy raw graph relationships into canonical knowledge.
16. Missing/revoked Hub access never disables local Graph, Review, Runtime, Data-flow or Quality Gate.
17. Never place Host Access, Project Access, Worker credentials or other secrets in Git. `.code-intel/local/` is local-only.
18. Use one Project Access per Project; record Agent client/version/time as metadata instead of creating a token per Agent.
19. Before ending the session, report the local Quality Gate, revision, checks, remaining warnings/unknowns, Hub sync state and Wiki status separately.
20. Load the conditional security reference only for an authorized security/API task. Validate before
    running; use only policy-declared lab/staging targets, actors and payloads.
21. Never perform production probing, denial-of-service, brute force, evasion, exfiltration, dynamic
    tool download or self-modifying security knowledge through Code Intelligence.
22. Security evidence is local-only, bounded and redacted. Missing matrix coverage is UNKNOWN/DENIED,
    not PASS.
23. Environment drift checks are offline and read-only. Updates require a reviewed, pinned, signed,
    rollback-capable release; analysis never installs or updates tools automatically.
24. When configured, correlate an executed security report to exact current-revision route symbols
    before verification. Missing/ambiguous routes are UNKNOWN, never absence.
25. Do not claim platform ROI from anecdotal output. Use paired baseline/platform tasks and include
    security finding recall, false passes, tokens, reads, tool calls and latency.

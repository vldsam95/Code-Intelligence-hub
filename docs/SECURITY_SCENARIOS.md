# Authorized security scenarios

Code Intelligence now supports a constrained runtime layer for validating API authorization and abuse
controls. It complements the source graph; it does not turn Code Intelligence into an unrestricted
penetration-testing framework.

## Design guarantees

- The MCP/CLI call accepts only a project path, ChangeRun id, action and predeclared scenario ids.
  The caller cannot inject a target, credential, header or payload.
- The read-only `inventory` action extracts bounded route/endpoint metadata from the current graph to
  seed the policy. Partial graph coverage remains an explicit unknown and cannot prove completeness.
- `validate` and `correlate` are non-networked. `run` requires a lab or explicitly authorized staging policy with an
  expiry and exact host allowlist. Lab mode accepts loopback hosts only.
- Redirects are disabled. Execution is capped at 100 requests by policy and normally far lower. A
  single scenario may repeat at most five times. Only `rapid_repeat` may opt into `parallel_burst`,
  with a hard cap of four workers, specifically to expose non-atomic quota/budget races.
- POST/PUT/PATCH/DELETE require approval at both policy and scenario level.
- Credentials are environment references. Inline secret-like JSON keys are rejected.
- Responses are bounded in memory. Local evidence stores status, latency, byte count and SHA-256 only.
  Security reports are not sent to Hub in this release.
- Runtime results are revision-bound and explicitly partial: unlisted endpoints and code paths remain
  unknown.
- Each scenario declares an exact `route_symbol` such as `GET /api/orders/{id}`. After a run,
  `correlate` binds its status/cost evidence to the matching route and a depth/node-bounded downstream
  graph. It returns no source bodies and reports missing or ambiguous routes as `UNKNOWN`.

## Policy example

Copy `templates/security-scenarios.policy.yaml` into the `security_scenarios` section of a project
policy and adapt it to disposable fixtures. Keep the default `plan_only` until the owner has recorded
authorization and created local actor environment variables. The starter is intentionally incomplete:
validation remains DENIED until every endpoint/case pair has a scenario or a substantive exclusion.

The cost probe reads an authoritative numeric usage counter before and after a scenario. This catches
the class where an application reserves zero budget for an unpriced model but still invokes a paid
provider. The expected cost delta must be defined from application policy, not inferred by the LLM.

## Evidence states

- `VALIDATED`: schema and safety contract are valid; no request was sent.
- `DENIED`: one or more hard boundaries are missing; no request was sent.
- `PASS`: every selected assertion passed for the current source revision.
- `FAIL`: at least one status, rate-limit, replay or cost assertion failed.
- Correlation `PASS`: every selected executed scenario matched one exact current-revision route;
  downstream gaps remain visible in the per-scenario unknowns.

`PASS` applies only to the selected matrix and declared fixtures. It is not a claim that the application
has no vulnerabilities.

## Updating tools and security knowledge

Code Intelligence performs no automatic downloads while analyzing a project. Run
`python3 scripts/audit-environment.py` to compare the local environment with the bundled reviewed
baseline. A stale baseline or version gap is reported for human review; the audit never installs or
updates anything. Release updates remain pinned, signed, verified and rollback-capable through the
existing updater.

## Proving that the platform adds value

Do not infer ROI from the existence of the graph or the runner. Use `benchmarks/benchmark.py` with
the same model, settings, starting revision and task in baseline/platform arms. Security tasks may
declare pre-reviewed `required_findings`; measurements return `finding_ids`, and the report compares
recall as well as false passes, tokens, raw reads, tool calls and latency. Expand only after 20–30
paired real tasks show no quality or security-recall regression.

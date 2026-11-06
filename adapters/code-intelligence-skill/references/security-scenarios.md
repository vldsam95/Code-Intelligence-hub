# Security scenarios (conditional guidance)

Load this reference only for an owner-authorized security review, an API/authentication change, or
when `policy.yaml` enables `security_scenarios`. Static review remains useful, but it is not evidence
that authorization, replay protection, rate limits or budget enforcement work at runtime.

## Hard boundary

Active requests require all of the following: an enabled Code Intelligence project; an active
ChangeRun; a predeclared policy matrix; `mode: lab` or `authorized_staging`; environment `lab` or
`staging`; authorization status `granted`; a non-empty scope reference; a future expiry; and an exact
allowlisted origin. `plan_only` validates configuration and sends no requests. Production, arbitrary
targets, redirects, inline credentials, hidden downloads, brute force, denial-of-service, evasion and
data exfiltration are outside this workflow.

Do not put credentials in prompts, tracked YAML, reports or Hub messages. Actor credentials are local
environment-variable references. Security evidence remains local and records only status, latency,
bounded byte count and a response hash—not response bodies, request secrets or query values.

Use `inventory` to obtain bounded endpoint metadata, then set each scenario's exact graph label in
`route_symbol` (for example `DELETE /api/users/{id}`). After `run`, call `correlate`; it performs no
network activity and links dynamic status/cost evidence to one exact route plus a bounded downstream
flow. A missing or duplicate route is `UNKNOWN`/`AMBIGUOUS`, never an absence claim.

## Required endpoint matrix

Inventory every externally reachable endpoint and map each applicable case. A complete endpoint
normally covers:

1. anonymous request
2. authenticated request as another user
3. lower-privilege role
4. another user's object identifier (BOLA/IDOR)
5. missing parameters
6. unexpected parameters (including mass assignment)
7. oversized but bounded values
8. a small rapid-repeat sequence with an expected rate-limit response
9. replay with one stable idempotency key
10. expired or revoked token
11. changed HTTP method
12. direct API access without the frontend

Document `not_applicable` cases with a reason. Never treat an omitted case as a pass. For destructive
methods, require both policy-level and scenario-level state-change approval and use disposable lab
fixtures. Keep repetition at five or fewer requests; this checks controls without becoming a load or
availability test.

For a suspected non-atomic quota or provider-budget check, `rapid_repeat` may set
`parallel_burst: true`. Keep `max_concurrency` at four or fewer and `repeat` at five or fewer. Require
at least one declared terminal rate-limit status; all-success responses fail the scenario. This is a
bounded atomicity probe, not a throughput, stress or denial-of-service test.

## Cost-abuse checks

For paid upstream calls, define a read-only local/staging cost probe around the scenario. The probe
reads one numeric JSON field (for example, metered usage) before and after the request sequence and
fails when the observed delta exceeds the declared maximum. Also assert that rejected anonymous,
foreign-object, replayed and rate-limited requests do not trigger billable upstream work. A `$0`
reservation is not proof that a paid call was prevented; measure the authoritative usage boundary.

## Evidence and completion

Prefer two independent sources for a validated finding: current-source/static flow evidence and a
revision-bound runtime scenario. Runtime proves only the declared matrix. Missing actors, coverage,
authorization, environment classification or current-revision evidence is `UNKNOWN`/`DENIED`, never
PASS. Run correlation when required, then review, verification and the exact-revision Quality Gate.

Do not claim that Code Intelligence outperforms direct source review without paired evidence. For a
representative security dataset, predeclare stable finding identifiers and compare baseline/platform
recall, false passes, tokens, file/search reads, tool calls and latency. Prefer 20–30 matched tasks;
freeze expansion when quality or finding recall regresses.

For secure implementation work, derive server-side policy from authenticated identity, authorize the
specific object, reject unknown fields, enforce limits before paid work, make idempotency atomic, and
record metering only at the authoritative billing boundary. Frontend checks are usability controls,
not authorization controls.

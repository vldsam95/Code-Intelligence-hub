---
name: code-intelligence
description: Enforce source-revision-aware preflight, review, verification, and quality gating for projects containing .code-intel/project.yaml.
---

# Code Intelligence operating workflow

Start with one cheap activation check. When `.code-intel/project.yaml` is absent or `enabled` is not
`true`, report `CODE_INTELLIGENCE_NOT_ENABLED` internally and continue normal Agent work. Never run
`codeintel init`, indexing, registration, runtime collection, or a Quality Gate unless the owner
explicitly requested onboarding. A project that only has an LLM-Wiki does not activate this workflow.

When `.code-intel/project.yaml` is enabled, open the project through the local Code Intelligence MCP.
Read the project-local `.code-intel/SKILL.md` when present. Local analysis never requires Hub access;
a missing or revoked Hub credential changes only registration/sync status.
Use graph results only for discovery and inspect the actual current source before a relevant edit.
Run `preflight_change` before significant changes. After editing, run `review_change`, every check
required by `verify_change`, and `quality_gate` for the exact current source revision. Never infer
absence from an empty graph result or an unobserved runtime path. Dead-code results are candidates
only. Report the gate, revision, checks, remaining warnings/unknowns, and Wiki update status in chat.
Report Hub sync separately. Never commit `.code-intel/local/` or any Host/Project/Worker credential.

For an owner-authorized security review, an API/authentication change, or an enabled
`security_scenarios` policy, read `references/security-scenarios.md` completely. Validate the matrix
before execution. Run only predeclared scenarios against an explicitly authorized lab/staging origin;
never accept targets, credentials or payloads through tool-call arguments. Treat omitted endpoints,
missing actors, unobserved paths and expired authorization as UNKNOWN/DENIED. Security evidence is
local-only and must not contain response bodies or secrets. When correlation is required, bind the
executed report to exact `route_symbol` entries before verification; ambiguity remains UNKNOWN.

Use Code Intelligence/current source for live technical truth. Load durable context from the Wiki only
when the manifest says `wiki.mode=local` or `wiki.mode=remote`; `wiki.mode=none` is fully supported and
a Wiki outage never blocks the code-quality pipeline.

The complete canonical policy is available from the Hub and through MCP initialize instructions.
Environment/source update checks are offline and advisory: use the bundled baseline and never install,
download, self-modify or register tools during project analysis. Critical gaps require human-reviewed,
pinned and rollback-capable release maintenance.

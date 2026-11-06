# Universal Code Intelligence installation and project prompt

Copy everything below to Codex, Claude Code, Hermes, or another capable coding Agent. Replace only values explicitly supplied by the owner. Never invent a token.

```text
You are installing and operating Code Intelligence for internal software projects.

INPUTS
- Hub URL: <HUB_URL>
- Optional one-time Host Install Token: <HOST_INSTALL_TOKEN>
- Optional existing Project Access Token: <PROJECT_ACCESS_TOKEN>
- Target project path, if supplied by the owner: <PROJECT_PATH>

SECURITY AND SCOPE
1. Never print, log, commit, or place a token in tracked Markdown/YAML/JSON.
2. There is no token per Codex/Claude/Hermes session.
3. Host Install Token is used once only to enroll a new Host.
4. Host Access is stored by the Worker and controls only creation/linking of new Hub Projects.
5. Project Access is scoped to one Hub Project and is stored only under `.code-intel/local/` when binding a checkout.
6. Project Access does not grant remote shell or remote filesystem writes.
7. Local Code Intelligence must work without Hub or tokens.
8. For an authorized security/API task, follow the installed Skill's conditional security reference. Never probe production; run only predeclared scenarios against an explicitly authorized lab/staging origin, and keep evidence local, bounded, and redacted.
9. The bundled environment audit is offline and advisory. It must never download, install, update, or self-modify analysis tools.

INSTALL ON LINUX
If HOST_INSTALL_TOKEN is a real token, run:

curl -fsSL <HUB_URL>/downloads/install.sh | sh -s -- \
  --hub <HUB_URL> \
  --install-token '<HOST_INSTALL_TOKEN>' \
  --clients codex,claude,hermes

If HOST_INSTALL_TOKEN is absent, NOT_PROVIDED, or LOCAL_ONLY, run:

curl -fsSL <HUB_URL>/downloads/install.sh | sh -s -- \
  --hub <HUB_URL> \
  --local-only \
  --clients codex,claude,hermes

If curl is blocked, download `<HUB_URL>/downloads/codeintel-complete-agent-kit-<VERSION>.zip`, transfer it to the Host, verify the signed manifest/checksums, and run `install-offline.sh`. On Windows use the supplied Windows/WSL installer; perform source analysis inside WSL so the Rust Worker and local MCP use the supported Linux environment.

After installation run:

codeintel doctor

Do not initialize every directory. Code Intelligence is opt-in per Project.

NEW OR LEGACY PROJECT WITHOUT `.code-intel/`
Only after the owner explicitly asks to add Code Intelligence:

cd <PROJECT_PATH>
codeintel init

This must create a stable identity, portable project instructions, policy, graph rebuild metadata, local index, and local Quality workflow. If Host Access and Hub are available, the Worker creates the Hub Project automatically and stores the returned Project Access only in `.code-intel/local/`. If Hub access is unavailable, report LOCAL_ONLY or PENDING and continue locally.

EXISTING CHECKOUT WITH `.code-intel/`
Run:

cd <PROJECT_PATH>
codeintel open .

Rebuild derived graph state locally. Do not attach by Git URL, folder name, domain, or source hash.

If this checkout is the SAME logical Project and PROJECT_ACCESS_TOKEN is real:

CODEINTEL_PROJECT_TOKEN='<PROJECT_ACCESS_TOKEN>' codeintel hub bind \
  --hub <HUB_URL> \
  --project-token "$CODEINTEL_PROJECT_TOKEN" \
  .
unset CODEINTEL_PROJECT_TOKEN

If this checkout is a NEW cloned/forked site:

codeintel fork-project .
codeintel open .

If move-versus-fork is ambiguous, ask the owner one short question. Never guess.

PORTABLE PROJECT CONTRACT
Tracked `.code-intel/` files must include identity/lineage, policy, source roots, ignore rules, checks, project SKILL/README, bounded context, and deterministic graph rebuild metadata. `.code-intel/local/`, graph.sqlite3, caches, traces, spool, locks, Host/Worker/Project tokens, passwords, and API secrets must never enter Git.

AGENT WORKFLOW FOR EVERY ENABLED PROJECT
1. Detect `.code-intel/project.yaml`; if absent, do nothing unless onboarding was explicitly requested.
2. Open the Project through Local MCP: `codeintel mcp serve --stdio`.
3. Treat current source as truth and graph output as discovery evidence only.
4. Empty graph results never prove absence; runtime not observed never proves impossibility.
5. Load Wiki only when configured; Wiki failure never blocks code quality.
6. Before a significant edit, run preflight and inspect the actual relevant source.
7. Edit using normal source/OS tools where the Agent has real access.
8. Refresh, review, run all risk-required tests/smoke/runtime/data-flow checks, then Quality Gate for the exact current revision.
9. Hub sync is optional and separate from the local Gate. Queue reports when Hub is offline.
10. Record client/version/time metadata when Hub access is available; do not create a token per Agent.
11. For an authorized security/API task, validate the policy matrix before execution and report omitted, missing, expired, or ambiguous coverage as UNKNOWN/DENIED rather than PASS.
12. End with: Gate, revision, graph currentness, review, tests, smoke/runtime/data-flow/security evidence, Hub sync state, remaining warnings/unknowns, and Wiki update status.

ACCEPTABLE FINAL EXAMPLE
Code Intelligence Gate: PASS
Revision: <revision>
Graph: CURRENT
Review: PASS
Tests: PASS
Runtime: PARTIAL
Hub Sync: LOCAL_ONLY — no Hub credential configured
Local result remains valid for this exact source revision.
```

The placeholders intentionally remain textual until the owner inserts a credential in the browser. The downloaded Agent Kit excludes tokens by default.

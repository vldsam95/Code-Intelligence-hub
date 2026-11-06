# Code Intelligence Platform — Final Production Specification

**Version:** 1.0 Final for Implementation
**Date:** 2026-08-17
**Purpose:** Binding contract for a developer/agent building the complete production system
**Human interface language:** English throughout the UI and documentation
**Status:** This document supersedes specification 0.9 and earlier extensions wherever they conflict

---

# 0. Binding implementation instruction

Build one independent, unified production product providing Codex CLI, Claude Code, Hermes and MCP-compatible agents with:

1. Focused, accurate code understanding.
2. Local code graphs and semantic resolution.
3. Pre-change impact analysis.
4. High-quality post-change review.
5. Risk-based tests, HTTP smoke checks and runtime evidence.
6. A quality gate that prevents false confidence.
7. Token-efficient context retrieval.
8. A central Hub for projects, Workers, code health, history, instructions, installation and tokens.
9. Structured integration with an LLM Wiki/knowledge hub without creating a duplicate code Wiki.
10. Clean migration between VPS hosts and computers without depending on GitHub or any particular Git remote.

The product is not a demo, a deliverable MVP or a collection of four external daemons. Implementation may proceed in internal stages, but delivery or production readiness must not be declared until every mandatory requirement and acceptance test in this document passes.

---

# 1. Intended operating environment

The system explicitly targets this environment:

- The user operates multiple VPS hosts and a personal computer.
- Codex, Claude and Hermes run on the servers, sometimes with root privileges and bypass mode.
- LLM agents are the primary development interface.
- The user does not perform manual code-level review.
- Many changes are made directly in a live production environment.
- GitHub may be used only as backup; remotes may change, disappear or move between repositories.
- VPS snapshots are taken manually outside the agent and this system.
- A central LLM Wiki/knowledge hub exists, sometimes alongside a local project Wiki.
- RAM is more constrained than storage; 20–30 GB may be free, but must not be treated as available for unrestricted consumption.
- Code quality and stability come first; token savings matter but never take precedence over quality.

---

# 2. Product goals and priorities

Required priority order:

```text
1. Correct source identity
2. No false confidence
3. Code-quality verification
4. Evidence provenance and freshness
5. Runtime stability and regression prevention
6. Token-efficient agent context
7. Low RAM and controlled disk use
8. Portability between hosts
9. Maintainability
10. UI convenience
```

Measure the product by:

```text
fewest defects + highest justified confidence + least unnecessary context
```

rather than:

```text
maximum number of analyzers
minimum tokens at any cost
prettiest graph
```

---

# 3. Final decisions on earlier proposals

## 3.1 Transactional/Shadow Workspace

A shadow workspace is not a mandatory core component or a prerequisite for PASS.

The default is:

```yaml
workspace:
  mode: live
```

Agents continue working on live code through their normal tools. The system surrounds that work with:

- source revision tracking;
- preflight;
- review;
- tests;
- smoke checks;
- runtime verification;
- quality gate;
- unverified-change detection.

Shadow/copy mode is an optional capability for projects that choose it:

```yaml
workspace:
  mode: live | auto_copy | reflink | rsync | local_git_worktree
```

There is no GitHub dependency. `local_git_worktree` uses local Git if present and does not depend on a remote. Reflink/rsync modes work without Git.

## 3.2 Snapshots and rollback

VPS snapshot creation remains a manual, external action. The system:

- Requires no VPS-provider access;
- Does not create snapshots;
- Does not perform restores;
- Does not block quality gates because a snapshot was not created;
- May retain an optional `manual_snapshot_reference` note only.

After a manual restore, however, the system must detect a changed or reverted source revision, invalidate the currentness of later evidence and rebuild the index.

## 3.3 Git and GitHub

GitHub is neither a dependency nor a source of truth or part of project identity.

The system must remain fully functional when:

- There is no GitHub remote;
- The remote changes;
- The project moves to another repository;
- The remote repository is deleted;
- Local Git is absent;
- The working tree is dirty;
- Code is copied manually between VPS hosts.

Git, when available, provides optional acceleration and metadata only.

## 3.4 Security Scope

The product is not a sandbox and does not manage agent permissions on the host. It does not confine Codex to a project directory or prevent access to Docker, databases, systemd, package managers or external directories.

OS security, source-level prompt injection, build-script scanning and supply-chain hardening are outside this specification's focus.

The minimum operational security requirements needed for an internet-accessible private Hub remain:

- TLS;
- Single-owner login;
- Separate tokens supporting rotation and revocation;
- Encryption of secrets stored in the Hub;
- Worker outbound-only;
- No full source in the Hub;
- No tokens in logs.

## 3.5 Production Scope

No partial MVP is delivered. Implementation stages are internal sequencing only. A production release requires every `REQUIRED FOR V1` capability and all acceptance tests to pass.

---

# 4. What the product is not

- A separate code Wiki.
- A UI whose primary purpose is an attractive graph.
- Another internal LLM reasoning in place of Codex.
- A router running four repositories with separate graphs.
- A product requiring GitHub Actions.
- A deployment or VPS snapshot system.
- An agent sandbox.
- A guarantee of 100% correctness.
- A system uploading full source to the Hub.
- A tool that automatically deletes dead code.

---

# 5. Reference capabilities and product unification

The four identified projects serve as reference implementations and requirements sources, not four services required in production.

## 5.1 CodeGraph — capabilities to preserve

- Tree-sitter deterministic parsing.
- SQLite/FTS-backed structural graph.
- Incremental synchronization and file watching.
- callers, callees, path, impact radius.
- framework-aware edges.
- Output sufficient for agents without reading dozens of files.
- Installers/adapters for multiple agent clients.
- Concise, focused MCP guidance.

## 5.2 Codebase Memory MCP — capabilities to preserve

- Native/low-infrastructure operation.
- broad parser architecture.
- Semantic/hybrid type resolution beyond AST structure alone.
- Route nodes and cross-service HTTP relationships.
- auto-sync.
- graph coverage queries.
- Runtime-trace ingestion to validate dynamic relationships.
- architecture summary.

## 5.3 Code Review Graph — capabilities to preserve

- delta review.
- blast radius.
- risk-scored change analysis.
- affected flows.
- test mapping and test gaps.
- minimal review context.
- token/context savings metrics.
- graph diff and architecture/coupling warnings.

## 5.4 Code-Graph-RAG — capabilities to preserve

- unified multi-language graph concepts.
- graph-based retrieval.
- semantic search as optional plugin.
- data-flow/reachability concepts.
- Dead Code candidates based on entry-point reachability.
- real-time graph refresh concepts.

Do not import a reference implementation's heavy dependencies into the core merely to reproduce its structure. Memgraph, Qdrant, Ollama and remote LLMs are not mandatory runtime dependencies.

## 5.5 Unified result

The final product must expose:

```text
One Hub
One Worker per host
One local MCP
One remote Hub MCP
One graph/evidence schema
One risk engine
One review engine
One quality gate
One installer
One agent-documentation center
```

---

# 6. High-level architecture

```text
                           CENTRAL HUB
     UI / API / Project Registry / Agent Center / Tokens / History
                  Durable Context Packs / Remote MCP
                                  │
                 Worker-initiated outbound HTTPS/WebSocket
                                  │
             ┌────────────────────┼────────────────────┐
             ▼                    ▼                    ▼
        Worker VPS 1         Worker VPS 2         Worker VPS 3/PC
             │                    │                    │
      Local MCP/CLI         Local MCP/CLI         Local MCP/CLI
             │                    │                    │
      Actual source          Actual source          Actual source
             │                    │                    │
       SQLite graph          SQLite graph          SQLite graph
       review/runtime        review/runtime        review/runtime
```

## 6.1 Hub = Control and Knowledge Plane

The Hub stores compact, centralized, durable information:

- Projects and project instances.
- Worker/Host registry.
- Source revision history.
- Code Health.
- Change runs and quality runs.
- Findings, warnings and unknowns.
- architecture/context summaries.
- Timestamps and current/stale semantics.
- Agent Setup docs.
- installer/downloads.
- API/MCP schemas.
- token management.
- Wiki bindings.
- remote query requests/results.

By default, the Hub does not retain:

- Full Source.
- Raw graph database.
- Raw runtime dumps.
- LSP caches.
- local build artifacts.

## 6.2 Worker = Live Analysis Plane

Install one Worker on each host where agents work with code. It:

- Detects projects.
- Reads actual source.
- Computes SourceRevision independently of Git.
- Builds and updates the graph.
- Provides semantic resolution.
- Runs review/risk/tests/smoke/runtime analysis.
- Provides local MCP and CLI interfaces.
- Stores derived data locally.
- Sends only structured reports to the Hub.
- Receives remote read-analysis jobs through outbound connections only.
- Continues working while the Hub is offline.

---

# 7. Source of truth and evidence model

The system must distinguish:

```text
1. CURRENT SOURCE TRUTH
   Files actually present in the project instance at analysis time.

2. STATIC DERIVED EVIDENCE
   AST, graph, import/call/reference relationships.

3. SEMANTIC EVIDENCE
   type resolution, LSP/hybrid resolver, framework resolution.

4. TEST/CHECK EVIDENCE
   tests, lint, typecheck, build, command checks.

5. BEHAVIORAL EVIDENCE
   HTTP smoke, service health, CLI scenarios.

6. RUNTIME OBSERVATION
   Paths/edges/spans observed under a defined workload.

7. DURABLE PROJECT KNOWLEDGE
   An LLM Wiki or project context pack.
```

Every piece of evidence includes:

```text
evidence_id
project_id
instance_id
source_revision_id
change_run_id (optional)
producer
producer_version
origin
confidence
coverage
unsupported_areas
unknowns
started_at
completed_at
policy_hash
```

## 7.1 Graph Distrust Invariants

- Graph results are evidence for discovery, never sufficient evidence for correctness.
- Before modifying a relevant component, inspect the actual current source.
- Absence from a graph is not evidence of absence from the application.
- `0 callers` is never permission to delete.
- Runtime not observed is not proof that a path cannot execute.
- Evidence from an old revision is not current evidence.
- Unsupported analysis is `UNKNOWN/PARTIAL`, never PASS.

Encode these rules in the core and tests, not just prompts.

---

# 8. Required domain model

Implement at least the following as first-class objects:

```text
Project
ProjectInstance
WorkerHost
SourceRevision
IndexSnapshot
ChangeRun
AnalysisRun
Evidence
Finding
TestRun
SmokeRun
RuntimeRun
DataFlowRun
QualityRun
ProjectContextPack
WikiBinding
AgentCredential
RemoteAnalysisRequest
RemoteAnalysisResult
RecoveryEvent
```

Do not implement the entire system as unstructured JSON blobs.

## 8.1 Project

A stable logical identity independent of host, path, domain or Git remote.

```text
project_id: UUID immutable
slug: human-readable and changeable
name: display name
```

## 8.2 ProjectInstance

A physical copy of a project:

```text
instance_id
project_id
host_id
primary_path
related_roots
current_source_revision_id
heartbeat
status
```

## 8.3 ChangeRun

A ChangeRun represents one code task without becoming a deployment transaction:

```text
change_run_id
project_id
instance_id
agent_client
agent_session_id (if available)
task_summary
before_source_revision_id
after_source_revision_id
risk_before
risk_after
required_checks
evidence_ids
finding_ids
quality_run_id
manual_snapshot_reference (optional only)
started_at
completed_at
status
```

States:

```text
OPEN
PREFLIGHTED
EDITING
REVIEWING
VERIFYING
GATED
CLOSED
ABANDONED
```

---

# 9. Git-independent project identity

## 9.1 Portable Manifest

At the primary project root:

```text
.code-intel/
├── project.yaml
├── policy.yaml
└── AGENT_POLICY.md     # project-specific additions only
```

`project_id` is a UUID created once and carried with the project.

## 9.2 Changing Git remotes

Changing `origin`, moving between GitHub/GitLab/private servers or deleting a remote does not change `project_id`.

## 9.3 Moving between VPS hosts

```text
copy/move source including .code-intel/
→ install/enroll Worker on new host
→ Worker reads same project_id
→ registers new ProjectInstance
→ computes source revision
→ rebuilds local derived indexes
→ Hub keeps all prior project history
```

## 9.4 Project Fork

When a copy becomes a new logical project:

```bash
codeintel fork-project
```

The command creates a new `project_id`, retaining an optional `parent_project_id` for lineage.

## 9.5 Missing manifest

If `.code-intel/` is lost during migration:

- The Hub displays a recovery wizard.
- The user/agent selects an existing project.
- A single-use rebind token is created.
- `codeintel recover-project --token ...` restores the manifest.
- A Git URL alone must not establish a final automatic match.

---

# 10. Git-independent SourceRevision and change detection

## 10.1 SourceRevision Algorithm

1. Enumerate source files using source roots and ignore policy.
2. Record each file's relative logical path, size, mode and SHA-256 of its bytes.
3. Sort deterministically.
4. Hash the inventory.
5. Include `policy_hash`, `indexer_version` and root-mapping version.
6. Retain Git metadata when available, but never use it as the sole identity.

```text
source_revision_id = SHA256(normalized file inventory + policy hash)
```

## 10.2 Ignore Rules

Defaults include at least:

```yaml
ignore:
  common:
    - .git/
    - .code-intel/local/
    - '*.log'
    - tmp/
    - cache/
    - logs/
  node:
    - node_modules/
    - .next/
    - dist/
    - build/
  php:
    - vendor/
    - bootstrap/cache/
    - storage/logs/
    - storage/framework/
  python:
    - __pycache__/
    - venv/
    - .venv/
    - '*.pyc'
  user_content:
    - uploads/
    - public/uploads/
    - storage/app/public/
```

Support include/exclude overrides. Never automatically exclude a directory merely because it resembles user content when the project explicitly declares source within it.

## 10.3 Diffs without Git

The primary diff engine compares two SourceRevisions using hash inventories.

For line/AST diffs, the Worker retains a local content-addressed compressed cache of indexed source text with deduplication and short retention. Never send this cache to the Hub.

Git diff is an optional accelerator only.

## 10.4 Safe File Reads

The indexer must avoid reading files midway through a write:

```text
stat before
→ read
→ stat after
→ if size/mtime changed: debounce and retry
→ after bounded retries: mark VOLATILE_DURING_READ
```

The watcher defaults to a 300–500 ms debounce.

## 10.5 Unverified Change

When the current source revision differs from the latest QualityRun revision:

```text
UNVERIFIED_CHANGE
```

Display this status on the main dashboard and project page. It is not `FAIL`, but the project must not appear as a current PASS.


---

# 11. Project manifest — proposed schema

```yaml
schema_version: 1
project_id: "1fd88cb5-5c0d-4e26-a433-2a01824581e9"
slug: "example-site"
name: "Example Site"
enabled: true

source:
  primary_root: "."
  related_roots:
    - path: "../shared-service"
      role: "shared_code"
      optional: true
    - path: "/etc/example-site"
      role: "runtime_config"
      index_mode: "metadata_and_selected_text"
  follow_symlinks: false
  git:
    mode: "metadata_optional"

wiki:
  mode: "remote"                 # none | local | remote
  wiki_id: "example-site"
  local_path: null

languages:
  auto_detect: true
  priority: [php, typescript, javascript, python]

frameworks:
  auto_detect: true

quality:
  auto_risk: true
  graph_is_advisory: true
  absence_requires_coverage: true
  dead_code_auto_delete: false
  final_chat_report: true
  max_auto_repair_cycles: 3

checks:
  test:
    commands:
      - name: "application-tests"
        command: ["bash", "-lc", "./vendor/bin/phpunit"]
        cwd: "."
        timeout_sec: 900
  lint:
    commands: []
  typecheck:
    commands: []
  build:
    commands: []

smoke:
  enabled: true
  base_url: "https://example.com"
  endpoints:
    - id: "home"
      method: GET
      path: "/"
      expect_status: [200]
    - id: "health"
      method: GET
      path: "/api/health"
      expect_status: [200]
  compare_latency: true
  body_assertions: []

runtime:
  enabled: true
  mode: "on_demand"
  adapters: [generic_trace_ingest, http_trace]
  raw_trace_retention_days: 3

remote_analysis:
  enabled: true
  allow_source_bodies: false
  allow_structured_live_queries: true

workspace:
  mode: "live"

resources:
  profile: "ram_constrained"
  max_heavy_jobs: 1
  max_lsp_processes: 1
  project_soft_mb: 768
  project_hard_mb: 1536
```

Store every secret, token, password and API key in host secrets or the Hub vault, never in the manifest.

---

# 12. Local Graph Store

## 12.1 Storage

Default:

```text
/var/lib/code-intelligence/projects/<project_id>/<instance_id>/
├── graph.sqlite3
├── content-cache/
├── semantic-cache/
├── runtime/
├── reports/
├── spool/
├── tmp/
└── locks/
```

SQLite uses WAL mode with migrations, integrity checks and transactional index publication.

Build new indexes in staging tables or a temporary database and publish atomically. A crash must never leave a partial graph marked CURRENT.

## 12.2 Unified Nodes

```text
PROJECT
ROOT
FILE
MODULE
NAMESPACE
PACKAGE
CLASS
INTERFACE
TRAIT
STRUCT
FUNCTION
METHOD
PROPERTY
VARIABLE
TYPE
ROUTE
ENDPOINT
MIDDLEWARE
JOB
QUEUE
EVENT
LISTENER
COMMAND
TEST
DATABASE_MODEL
TABLE_REFERENCE
CONFIG_KEY
EXTERNAL_SERVICE
DOCKER_SERVICE
```

## 12.3 Unified Edges

```text
CONTAINS
IMPORTS
EXPORTS
CALLS
REFERENCES
INHERITS
IMPLEMENTS
USES_TRAIT
INSTANTIATES
OVERRIDES
ROUTES_TO
USES_MIDDLEWARE
DISPATCHES
LISTENS_TO
SCHEDULES
READS_CONFIG
WRITES_TO
READS_FROM
TESTS
HTTP_CALLS
ASYNC_CALLS
FLOWS_TO
RUNTIME_CALLS
CHANGED_WITH
```

## 12.4 Edge Provenance

Every edge includes:

```text
origin:
  STATIC_EXACT
  STATIC_HEURISTIC
  SEMANTIC_RESOLVED
  LSP_RESOLVED
  FRAMEWORK_RESOLVED
  CONFIG_RESOLVED
  RUNTIME_OBSERVED
  TEST_OBSERVED

confidence
source_file
source_range
source_revision_id
analyzer
analyzer_version
ambiguity_reason
last_seen_at
```

Never present a heuristic edge as exact.

---

# 13. Parser, semantic resolution and framework awareness

## 13.1 Parsing

Tree-sitter is a recommended foundation, but required outcomes matter more than the library.

`REQUIRED FOR V1` deep support:

- PHP.
- JavaScript.
- TypeScript/TSX.
- Python.

Generic structural support must extend to additional languages. Languages without deep support receive explicit coverage limits, never a false PASS.

## 13.2 SemanticResolver Interface

```text
SemanticResolver
- detect(project)
- resolve_definition(location)
- find_references(symbol)
- resolve_type(location)
- resolve_call_target(call_site)
- coverage()
- shutdown()
```

Implementation options:

- embedded hybrid resolver;
- external LSP;
- A combination of both.

Required outcomes:

- Resolution beyond textual name matching.
- Retained provenance.
- Preserve ambiguity rather than inventing a single target.
- Start external LSPs lazily, with at most one running at a time by default.

## 13.3 FrameworkAdapter Interface

```text
FrameworkAdapter
- detect()
- extract_routes()
- extract_entry_points()
- extract_container_bindings()
- extract_jobs_events_schedulers()
- extract_models_and_migrations()
- discover_test_commands()
- identify_dynamic_boundaries()
- risk_zones()
```

Priority adapters:

- Laravel/Composer/Blade.
- WordPress plugins/themes.
- Express/Node.
- Next.js routing where relevant.
- FastAPI.
- Django.
- Docker/Compose service relationships.

Add Ruby/Rails and other languages according to actual inventory through the same contract.

## 13.4 Related Roots

Agents are not confined to the project root. For intelligence, projects may declare related roots such as:

- A shared library outside the project directory.
- Docker Compose files.
- deployment config.
- reverse proxy config.
- service unit.
- sibling backend/frontend.

Each related root has a separate role, ignore policy and coverage.

---

# 14. Search, retrieval and token efficiency

## 14.1 Retrieval Order

```text
1. exact symbol/qualified name
2. exact path/config/route
3. FTS5/BM25
4. graph neighborhood
5. semantic resolver
6. optional embeddings only if benchmarked and enabled
```

## 14.2 Context Contract

Return the following first:

```text
summary
risk
relevant symbols
relevant files/ranges
why each item matters
coverage
unknowns
next recommended tool/action
```

Return source bodies only when needed. Agents can read files directly.

## 14.3 Context Handles

Large answers return a handle:

```json
{
  "summary": "...",
  "context_handle": "ctx_...",
  "items_returned": 12,
  "items_remaining": 41,
  "next": "request_detail"
}
```

This avoids returning identical information repeatedly.

## 14.4 Deduplication

- Do not return a snippet with the same `content_hash` twice in one ChangeRun unless requested.
- Repeated graph neighborhoods receive cached handles.
- Do not inject documentation again on every tool call.
- The Hub and Worker return versioned ETags/handles.

## 14.5 Context Budgets

Default targets:

```text
Global Skill/instructions loaded at session start: <= 1,200 tokens
Project Context summary: <= 2,000 tokens
Normal MCP response: <= 1,500 tokens
Minimal response: <= 500 tokens
Deep response: paginated, never unbounded
```

Measure using supported clients' actual tokenizers rather than character counts alone.

## 14.6 Token Savings Reality

- Indexing, graph rebuilding, hashing and runtime collection do not themselves consume LLM tokens.
- They consume CPU, disk I/O and sometimes RAM.
- Tool output and agent reasoning do consume tokens.
- Medium/large projects should save tokens when targeted context replaces file-by-file exploration.
- Tiny changes may incur modest preflight/review/gate overhead.
- Deep validation may consume more tokens than working without the tool; it is justified only when it provides required quality/confidence.

The KPI is:

```text
quality per token
```

rather than `tokens only`.

---

# 15. MCP Tool Surface — Local Worker

Expose only 8–10 primary tools.

## 15.1 `open_project`

- Detect the manifest from cwd or an explicit path.
- register/open ProjectInstance.
- verify freshness.
- incremental refresh.
- return readiness/current revision.

## 15.2 `get_project_context`

Returns a brief context pack:

- project identity.
- languages/frameworks.
- current revision.
- active policy.
- linked Wiki.
- current health.
- important architecture.
- unresolved risks.

## 15.3 `understand`

Intent-based high-level retrieval:

- flow.
- architecture area.
- feature.
- route.
- integration.
- data path.

## 15.4 `find_code`

- symbol.
- references.
- callers/callees.
- route/config/test.
- path between symbols.
- source ranges.

## 15.5 `preflight_change`

- starts/continues ChangeRun.
- captures before revision.
- identifies affected areas.
- computes risk.
- returns required checks.
- optionally captures baseline smoke/tests.
- pins local content cache needed for later diff.

## 15.6 `review_change`

- compares before/after revision.
- structural diff.
- blast radius.
- caller/route/job/test impact.
- new unresolved references.
- test gaps.
- risk delta.
- repair suggestions.

## 15.7 `verify_change`

Unified verification router:

- tests/lint/typecheck/build.
- HTTP smoke.
- service/CLI checks.
- semantic escalation.
- runtime trace.
- data-flow where required.

The risk engine selects required checks; users do not choose NORMAL/STRICT/DEEP.

## 15.8 `quality_gate`

A deterministic final verdict for the current source revision.

## 15.9 `project_status`

- current/stale.
- index status.
- last gate.
- unverified change.
- storage/RAM.
- active lock/change run.
- Hub sync.

## 15.10 Advanced Namespace

```text
advanced.graph_query
advanced.find_path
advanced.edge_evidence
advanced.coverage
advanced.dead_code_audit
advanced.dataflow
advanced.runtime_ingest
```

Hide advanced tools from default agent context when clients support tool filtering.

---

# 16. Agent activation, skill and client adapters

## 16.1 Activation

The workflow activates when:

- `.code-intel/project.yaml` exists with `enabled: true`; or
- The user/agent explicitly requests `codeintel init/open`.

Do not impose code workflows on SEO, spreadsheet, finance or other non-code tasks.

## 16.2 Global Skill

The installer adds a concise skill for each supported client, instructing it to:

1. Check the marker in code projects.
2. Open the project through local MCP.
3. Load context as needed.
4. Run preflight before significant changes.
5. Read actual source.
6. After changes, run review, required verification and a quality gate.
7. Do not finish with FAIL or blocking UNKNOWN.
8. Report the result in chat.

## 16.3 Client Adapters

Required:

- Codex CLI.
- Claude Code.
- Hermes.
- Generic MCP client.

Do not hard-code a single configuration path. At installation time, adapters detect the client version and supported configuration location.

## 16.4 Single Source of Agent Instructions

Maintain canonical policy once and expose it through:

- MCP initialize instructions.
- Hub rendered docs.
- raw Markdown endpoint.
- downloadable Skill.

Never duplicate policy manually across unsynchronized templates.

---

# 17. Standard Agent Workflow

```text
START TMUX / AGENT SESSION
↓
Agent receives user task
↓
Detect .code-intel project
↓
open_project
↓
get_project_context
↓
Load LLM-Wiki only if configured/relevant
↓
preflight_change(intent)
↓
Read actual relevant source
↓
Agent edits using its normal unrestricted shell/file tools
↓
Worker refreshes current source revision
↓
review_change
↓
verify_change runs required checks
↓
quality_gate
↓
FAIL or blocking UNKNOWN?
  yes → Agent fixes/gathers evidence → repeat review/verify/gate
  no  → final report
↓
Update durable Wiki knowledge only if warranted
↓
User's normal Git backup and manual VPS Snapshot workflow
↓
END SESSION
```

## 17.1 Final Chat Report

Before completing any code task, agents must report in chat:

```text
Quality Gate: PASS / PASS_WITH_WARNINGS / UNKNOWN / FAIL
Source revision:
What changed:
What was checked:
Tests:
Smoke checks:
Runtime/data-flow, if run:
Remaining warnings/unknowns:
Wiki updated: yes/no/not needed
```

A Hub report alone is insufficient.

---

# 18. ChangeRun, locking and concurrency

## 18.1 Analysis locks and agent freedom

Do not restrict agents at OS level. Do prevent concurrent analysis mutations on the same ProjectInstance.

The Worker uses an advisory per-project lock for:

- index publication.
- ChangeRun mutation.
- quality gate computation.
- content-cache pinning.

Read-only graph queries may run concurrently against a published snapshot.

## 18.2 Multiple Agents

When another agent opens an instance during an active ChangeRun:

- It receives `IN_USE` status.
- It may perform read-only exploration.
- It receives a clear warning before starting another ChangeRun.
- Policy may permit concurrent runs; the default is one.

## 18.3 External Changes

When files change outside a ChangeRun:

- The watcher updates the revision.
- The current gate becomes STALE.
- The Hub shows `UNVERIFIED_CHANGE`.
- An active ChangeRun records source drift and requires a refresh.

---

# 19. Automatic Risk Engine

The risk engine is as deterministic as practical and requires no internal LLM.

## 19.1 Inputs

- planned/changed files.
- changed symbols.
- public API/routes.
- blast radius.
- graph centrality.
- auth/security-sensitive paths.
- payment/billing.
- DB writes/migrations/schema.
- shared utilities.
- queues/jobs/events/cron.
- config/env.
- external integrations.
- framework dynamic dispatch.
- test availability/gaps.
- semantic ambiguity.
- graph coverage.
- source/graph contradictions.
- previous failures and false-pass feedback in the affected area.
- runtime/data-flow relevance.

## 19.2 Internal Tiers

```text
TIER_0 local/trivial
TIER_1 normal
TIER_2 elevated
TIER_3 dynamic/critical
```

Users do not need to see or select tiers.

## 19.3 Output

```json
{
  "risk_score": 78,
  "risk_level": "HIGH",
  "reasons": [
    "public route changed",
    "database write path",
    "semantic resolution partial"
  ],
  "required_checks": [
    "actual_source_verify",
    "semantic_resolution",
    "targeted_tests",
    "http_smoke",
    "runtime_trace"
  ]
}
```

## 19.4 Example Routing

- CSS literal: parse, source read, targeted visual/HTTP smoke if configured, review, gate.
- Shared utility: impact, callers, mapped tests, review, gate.
- Auth/API/DB: semantic resolution, tests, smoke, runtime/data-flow as applicable.
- Dynamic callback/reflection: runtime evidence if claim depends on runtime behavior.

---

# 20. Review Engine

Review combines deterministic structural checks with host-agent reasoning over focused context.

## 20.1 Deterministic Review Checks

- parse/syntax regressions.
- added/removed/changed symbols.
- public signature changes.
- unresolved imports/references.
- callers not adapted.
- route/endpoint changes.
- middleware/auth changes.
- job/event/listener/scheduler changes.
- config/env keys added or removed.
- model/schema/migration impact.
- tests affected/missing.
- new cycles or high-degree coupling.
- architecture boundary changes.
- dead/orphan candidates.
- source/graph contradictions.
- changed related roots.

## 20.2 Review Bundle

The host agent receives a compact bundle:

```text
change summary
actual diff ranges
impacted symbols/files
critical callers and routes
mapped tests
test gaps
risk reasons
runtime relevance
known unknowns
```

Never send the whole repository.

## 20.3 Repair Loop

`review_change` returns structured findings with:

```text
severity
category
path/range
why it matters
evidence
suggested repair
blocking
```

Agents fix issues and repeat review/verification/gating. After `max_auto_repair_cycles` (default 3), they must explicitly report FAIL/UNKNOWN with an explanation rather than hide unresolved defects.


---

# 21. Tests, builds, lint and command evidence

## 21.1 Agents remain unrestricted

Agents may run normal host commands, install runtimes and change Docker, databases or systemd as the task requires. Code Intelligence is not a sandbox.

## 21.2 Repeatable Checks

For comparable evidence, register stable commands in `policy.yaml` or through the UI:

```yaml
checks:
  commands:
    - id: unit-tests
      kind: test
      argv: ["pytest", "-q"]
      cwd: "."
      timeout_sec: 900
      required_for: [MEDIUM, HIGH, CRITICAL]
    - id: typecheck
      kind: typecheck
      argv: ["npm", "run", "typecheck"]
      cwd: "."
      timeout_sec: 600
```

The system may execute commands or accept structured agent results, but must retain:

- command/argv.
- working directory.
- exit code.
- start/end time.
- bounded stdout/stderr summary.
- source revision.
- An environment-profile hash without secret values.

## 21.3 Existing Failures Baseline

Where possible, `preflight_change` runs a baseline of critical checks.

Post-change verdicts distinguish:

- pre-existing failure.
- new regression.
- fixed failure.
- environment/tool failure.

New regressions block PASS. Pre-existing failures are displayed explicitly, never hidden.

## 21.4 No Tests

Having no tests does not mean no verification is required.

- A tiny change may receive PASS with sufficient alternative evidence under policy.
- A high-risk behavioral change without behavioral/test evidence receives UNKNOWN, never PASS.

---

# 22. HTTP smoke checks, service health and scenarios

HTTP smoke checks are `REQUIRED FOR V1` for web applications and do not require heavy runtime tracing.

## 22.1 HTTP Smoke Adapter

```yaml
smoke:
  enabled: true
  base_url: "https://example.com"
  endpoints:
    - id: home
      method: GET
      path: "/"
      expect_status: [200]
      max_latency_ms: 3000
    - id: login
      method: GET
      path: "/login"
      expect_status: [200]
    - id: health
      method: GET
      path: "/api/health"
      expect_status: [200]
```

Optional assertions:

- header exists/value pattern.
- body contains stable marker.
- JSON path/schema.
- redirect target.
- latency regression.

Full-body hashing is disabled by default to avoid noise from dynamic pages.

## 22.2 Before/After

```text
preflight baseline smoke
→ edit
→ post-change smoke
→ compare
```

An endpoint passing before a change and failing afterward creates a blocking finding and FAIL.

## 22.3 Generic Service Checks

A generic adapter for:

- local HTTP port.
- systemd service active.
- Docker/Compose service health.
- CLI command returns expected status.
- read-only DB connectivity check.

## 22.4 Scenario Checks

Complex flows such as login, checkout and import can be configured as scripted scenarios. They are optional per project, but the risk engine may require existing scenarios when relevant.

---

# 23. Runtime Observation

Runtime is supplementary evidence, not absolute truth.

## 23.1 RuntimeAdapter Contract

```text
RuntimeAdapter
- detect(project)
- prepare(workload)
- start_or_attach()
- collect()
- normalize()
- coverage()
- cleanup()
```

## 23.2 Required V1 Runtime Modes

1. `generic_trace_ingest` — Ingest structured traces from JSON/NDJSON/OTLP or adapter formats.
2. `http_trace` — Correlate requests/routes/services using spans/trace IDs when available.
3. `command_wrapped_runtime` — Run a defined workload under a temporary collector.
4. Plugin interfaces for Python, Node.js and PHP.

Never run a heavy profiler continuously. Collectors start for a run, terminate afterward and obey timeouts/resource caps.

## 23.3 Normalized Runtime Edges

```text
RUNTIME_CALLS
HTTP_CALLS
ASYNC_CALLS
RUNTIME_READS_FROM
RUNTIME_WRITES_TO
RUNTIME_DISPATCHES
```

Every edge includes:

```text
runtime_run_id
source_revision_id
workload
observed_at
count
coverage_scope
adapter/version
```

## 23.4 Runtime Semantics

```text
observed = evidence that path executed
not observed = only NOT_OBSERVED
```

Never return DOES_NOT_EXIST merely because a workload did not exercise a path.

## 23.5 Runtime UI

```text
Runtime: PARTIAL
Workload: checkout smoke scenario
Observed: HTTP route → CheckoutService → Payment client
Not exercised: admin import, nightly scheduler
Source revision: ...
```

---

# 24. Data-flow / Taint-like Analysis

The goal is practical reachability, not formal verification.

Internal API:

```text
flow_verdict(source, sink, scope)
```

Verdicts:

```text
FOUND
NO_FLOW
UNKNOWN
```

`NO_FLOW` requires a fully supported and covered declared scope. Dynamic/unsupported areas produce UNKNOWN.

Sources/Sinks examples:

- user input → SQL write.
- external payload → file write.
- secret/config → log/output.
- route → payment call.
- import source → production database.

Present static graphs, semantic edges and runtime observations separately and as overlays.

---

# 25. Dead Code Audit

Dead-code detection is advanced but `REQUIRED FOR V1` as a safe candidate mechanism.

## 25.1 Evidence

- static callers/references.
- semantic references.
- public/exported API.
- routes.
- framework registrations.
- config references.
- events/listeners.
- jobs/cron/commands.
- tests.
- runtime observations.
- reflection/dynamic indicators.

## 25.2 Output

```json
{
  "symbol": "...",
  "status": "CANDIDATE",
  "confidence": 0.71,
  "evidence": {},
  "unknowns": ["scheduler workload not observed"],
  "auto_delete_allowed": false
}
```

## 25.3 Removal Workflow

```text
candidate
→ read actual source
→ inspect framework/config registrations
→ inspect static and semantic references
→ runtime evidence where feasible
→ Agent decides
→ edit/remove
→ tests/smoke
→ review
→ quality gate
```

No function-by-function human approval is required, but the system must have no code path that deletes source automatically.

---

# 26. Quality Gate

## 26.1 States

```text
PASS
PASS_WITH_WARNINGS
UNKNOWN
FAIL
```

Currentness is a separate dimension:

```text
CURRENT
STALE
UNVERIFIED_CHANGE
```

## 26.2 PASS

Only when:

- All required evidence matches the current source revision.
- All required checks have completed.
- There are no blocking findings.
- Required tests/checks passed.
- Required smoke checks passed.
- Required runtime/data-flow analysis completed.
- There is no blocking UNKNOWN.

## 26.3 PASS_WITH_WARNINGS

- Required checks passed.
- Non-blocking warnings/unknowns remain.
- Every warning is displayed explicitly.

## 26.4 UNKNOWN

- A required tool is unavailable.
- Coverage is partial for a claim requiring proof of absence.
- Required runtime analysis cannot be executed.
- Source changed during analysis.
- graph stale/invalid.
- A critical area uses an unsupported language/framework.
- A high-risk change lacks sufficient behavioral evidence.

## 26.5 FAIL

- new test regression.
- A smoke endpoint broke.
- unresolved critical reference.
- blocking review finding.
- required invariant broken.
- data-flow violation.
- required check failed.

## 26.6 Meaning of PASS

Display a persistent explanation in the UI:

> PASS means all checks and evidence required by policy passed for the current source revision. It is not a mathematical guarantee that all business logic is correct.

---

# 27. Project context pack — central context without source storage

This capability lets agents on other computers understand projects whose source resides on another VPS.

## 27.1 Content

A context pack includes:

- project purpose/summary.
- languages/frameworks.
- active and historical instances.
- current/last-known source revision.
- currentness and Code Health.
- architecture summary.
- important entry points and business/technical flows.
- external services.
- known risk areas.
- recent validated changes.
- open findings/unknowns.
- test/smoke/runtime coverage summary.
- Wiki binding and links.
- instructions for the next Agent.

It contains neither full source nor raw graph dumps.

## 27.2 Generation

A context pack is built from:

- structured Worker reports.
- validated architecture summaries.
- Quality history.
- optional LLM-Wiki context.
- Durable technical notes approved after a quality gate.

## 27.3 Freshness

Every section is labeled:

```text
source_revision_id
produced_at
CURRENT / STALE / LAST_KNOWN
producer
```

## 27.4 Hub-only Questions

A remote agent can answer Hub-only questions such as:

- What is the latest health status?
- Which VPS runs the project?
- Which languages/frameworks are present?
- What is the high-level architecture?
- What changed recently?
- Which risks/unknowns remain open?
- Which checks passed?
- Is this information current?

Precise line-level questions about current code require a live Worker or direct source access.

---

# 28. Remote agent access and live analysis relay

## 28.1 Remote Hub MCP

Agents on other computers can connect to Hub MCP using project-scoped tokens.

Tools:

```text
list_projects
get_project_context
get_code_health
get_quality_history
get_findings
get_recent_changes
ask_project_context
request_live_analysis
get_agent_setup
```

`ask_project_context` returns structured context for the host LLM; the Hub does not need an internal LLM.

## 28.2 Live Worker Relay

When the Worker is active:

```text
Remote Agent
→ Hub MCP
→ structured live-analysis request
→ Hub queue/outbound channel
→ active Worker executes locally
→ structured result returned to Hub
→ Remote Agent receives result
```

Allowed live query types:

- project status/currentness.
- architecture refresh.
- find symbol metadata.
- callers/callees/impact.
- coverage.
- quality status.
- read-only preflight preview.

By default, return symbols, paths, ranges, summaries and evidence rather than source bodies. Future per-project snippet support is possible but unnecessary for the core product.

## 28.3 Worker Offline

The Hub returns last-known context with a warning:

```text
LIVE WORKER OFFLINE
Last synchronized: ...
Last source revision: ...
Freshness: STALE/LAST_KNOWN
```

Never present last-known answers as current.

## 28.4 Network Model

- The Worker initiates outbound HTTPS long polling or WebSocket connections.
- No public inbound Worker port.
- Local MCP keeps working while the Hub is down.
- Reports/jobs remain in the spool and synchronize later.

---

# 29. LLM-Wiki / Knowledge Hub Integration

## 29.1 Separation of responsibilities

```text
Code Intelligence:
What does current code do, what changed, what was verified and what is its quality status?

LLM-Wiki:
What do we know about the project, why is it designed this way, what was decided and what should be remembered?
```

Do not create another code Wiki copying thousands of symbols/edges.

## 29.2 Modes

```yaml
wiki:
  mode: none | local | remote
```

## 29.3 Durable Knowledge

Store in the Wiki:

- architecture stable.
- business/technical invariants.
- design rationale.
- important integrations.
- operational gotchas.
- validated consequence of significant change.
- residual risk worth remembering.

Do not store:

- raw callers lists.
- full graph.
- transient runtime trace.
- cache.
- every file/symbol relationship.

## 29.4 Context packs versus Wikis

A project context pack is an operational summary derived from system state. It does not replace a canonical Wiki.

When a Wiki is bound:

- The context pack includes a pointer and summary.
- Durable narrative stays in the Wiki.
- Live code relationships stay in Code Intelligence.

Without a Wiki:

- The Hub still maintains a minimal technical context pack so remote agents can understand status/history.

## 29.5 Knowledge Hub Capability

```json
{
  "capabilities": {
    "code_intelligence": {
      "enabled": true,
      "project_id": "...",
      "hub_url": "..."
    }
  }
}
```

A non-code Wiki without the capability receives no Code Intelligence instructions.

## 29.6 Update Flow

```text
quality_gate successful
→ Agent asks: did this create durable knowledge?
→ if yes: apply Wiki update through existing Knowledge Hub API/MCP
→ if no: no update just to tick a box
```

Remote Wiki failure does not block the code-quality pipeline; updates may become pending.

---

# 30. Central Hub UI — complete structure

The UI serves owners who should not need to read raw graphs to make decisions.

## 30.1 Global Tabs

### Dashboard

- Projects table.
- active instance.
- Current/STALE/UNVERIFIED.
- last Quality Gate.
- tests/smoke/runtime summary.
- open FAIL/UNKNOWN.
- Worker online/offline.
- updated timestamp.

### Projects

- create/register/recover/retire.
- filter by status/host/framework.

### Hosts & Workers

- host name/ID.
- Worker version.
- online/last heartbeat.
- registered projects.
- RAM/disk usage.
- installed Agent adapters.
- upgrade status.

### Agent Center

- Generic Agent.
- Codex CLI.
- Claude Code.
- Hermes.
- MCP manual.
- Skills.
- project bootstrap.
- remote agent setup.
- Knowledge Hub integration.

### Downloads & Install

- platform binaries/packages.
- installer scripts.
- checksums.
- release notes.
- one-line install command.
- enrollment token generator.
- downloadable docs/skills bundle.

### API & MCP

- OpenAPI UI.
- raw OpenAPI JSON.
- Hub MCP schema.
- Worker MCP schema.
- examples.
- version compatibility.

### Tokens & Connections

- owner/API tokens.
- Worker enrollment tokens.
- Worker credentials.
- Project-scoped Agent tokens.
- remote analysis tokens.
- Wiki connection credentials.
- reveal/copy/rotate/revoke.
- expiry and last-used.

### Documentation

- rendered Markdown.
- raw Markdown.
- version selector.
- search.
- ZIP download.

### System Health

- Hub API/UI/DB health.
- job queue.
- Worker sync failures.
- storage.
- migrations.
- logs/events.

### Settings

- owner account.
- token policy.
- retention.
- default resource profiles.
- client adapter defaults.

## 30.2 Per-Project Tabs

1. **Overview / Code Health**
2. **Architecture & Context**
3. **Changes & Quality History**
4. **Review Findings**
5. **Tests & Smoke**
6. **Runtime & Data-flow**
7. **Dead Code Candidates**
8. **Instances & Hosts**
9. **Knowledge / Wiki Binding**
10. **Agent Setup**
11. **Policy & Checks**
12. **Storage & Indexes**
13. **Advanced Graph**

## 30.3 Code Health Required Display

```text
Current source revision
Current/STALE/UNVERIFIED
Active instance
Git metadata if available
Last static index
Last semantic enrichment
Last review
Last tests
Last smoke
Last runtime
Last data-flow
Last quality gate
Open FAIL
Open blocking UNKNOWN
Open warnings
Last sync time
```

Every item includes a timestamp and source revision.

## 30.4 Architecture View

Defaults are human-friendly:

```text
HTTP Routes → Controllers → Services → Database
Background Jobs → Import Pipeline → Database
External APIs → Payments / Analytics
```

Raw symbol graphs are available under Advanced only.


---

# 31. Agent Documentation Center

This is a core product requirement, not incidental documentation.

## 31.1 Required Markdown documents

The Hub provides rendered UI and raw Markdown for:

```text
START_HERE.md
PRODUCT_CONCEPT.md
QUALITY_SEMANTICS.md
CODEX_CLI_SETUP.md
CLAUDE_CODE_SETUP.md
HERMES_SETUP.md
GENERIC_MCP_SETUP.md
LOCAL_WORKER_INSTALL.md
PROJECT_INITIALIZATION.md
REMOTE_AGENT_SETUP.md
AGENT_OPERATING_POLICY.md
LLM_WIKI_INTEGRATION.md
PROJECT_MIGRATION.md
SNAPSHOT_RESTORE_BEHAVIOR.md
TOKEN_MANAGEMENT.md
TROUBLESHOOTING.md
UPGRADE_ROLLBACK.md
```

## 31.2 Project-specific Generated Docs

Each project's Agent Setup tab generates:

- copyable Start Working prompt.
- project-specific MCP config.
- project token creation button.
- Wiki binding instructions.
- current policy summary.
- known commands/checks.
- remote-agent instructions.
- install/init command for current host.

## 31.3 Machine-readable Bootstrap

```http
GET /api/v1/agent-bootstrap?client=codex&project_id=<id>
```

Response:

```json
{
  "client": "codex",
  "project_id": "...",
  "worker_install": {...},
  "mcp_config": {...},
  "skill_markdown": "...",
  "start_prompt": "...",
  "wiki": {...},
  "policy_version": "...",
  "compatibility": {...}
}
```

## 31.4 Documentation Versioning

Every release includes:

```text
product_version
agent_policy_version
worker_mcp_schema_version
hub_mcp_schema_version
project_manifest_schema_version
```

Display mismatches in the UI; never hide them.

---

# 32. Downloads, installer and updates

## 32.1 Installation does not depend on GitHub

The Hub hosts or links release artifacts. Source may reside on any Git provider; installing a Worker must not require users/agents to clone from GitHub.

## 32.2 Required Artifacts

- Linux amd64 Worker.
- Linux arm64 Worker.
- install.sh.
- uninstall script.
- checksums.
- systemd unit.
- Agent adapter bundle.
- raw docs ZIP.
- Optional macOS builds according to the user's environment.

## 32.3 One-line Install

The Hub generates a prepared command with a short-lived enrollment token:

```bash
curl -fsSL https://<hub>/downloads/install.sh | sh -s -- \
  --hub https://<hub> \
  --enroll-token <ONE_TIME_TOKEN> \
  --clients codex,claude,hermes
```

## 32.4 Installer Responsibilities

- detect OS/arch.
- download correct binary.
- verify checksum.
- create `/etc/code-intelligence`.
- create `/var/lib/code-intelligence`.
- install systemd service.
- enroll Worker.
- detect installed Agent clients.
- install/update MCP configs and Skill adapters idempotently.
- preserve unrelated existing client configuration.
- run `codeintel doctor`.
- report result to Hub.

## 32.5 Project Init

```bash
cd /path/to/project
codeintel init
```

Wizard/Agent detects:

- language/framework.
- source roots.
- ignore rules.
- tests/build/lint commands.
- likely HTTP endpoints.
- Wiki binding.
- resource profile.

It creates the manifest/policy and builds the initial index.

## 32.6 Migration Install UX

On the new host:

```bash
codeintel open /path/to/moved-project
```

If the manifest exists, do not create a new project.

## 32.7 Upgrade

Worker upgrade:

```text
download + checksum
→ stop service
→ atomic binary replacement
→ DB schema migration transaction
→ start
→ doctor
→ mark incompatible indexes REBUILD_REQUIRED
→ lazy rebuild
```

Hub upgrades include database backup, migration and rollback paths.

---

# 33. Token and Credential Management UI

Users need convenient controls to view, copy and rotate credentials.

## 33.1 Token Types

```text
WORKER_ENROLLMENT      short-lived, one-time
WORKER_CREDENTIAL      per host, revocable
AGENT_HUB_READ         read central context/history
AGENT_LIVE_ANALYSIS    may request read-only live Worker queries
PROJECT_AGENT          scoped to one project
ADMIN_API              owner automation
WIKI_CONNECTION        connector credential/reference
```

## 33.2 UI Behavior

For every token:

- label.
- scope.
- created/expiry.
- last used.
- reveal.
- copy.
- rotate.
- revoke.
- test connection.
- ready config/command snippet.

Mask tokens by default. Owners may Reveal/Copy after reauthentication. To support repeated Reveal, store token values encrypted at rest rather than only hashed. Single-use tokens may be displayed only when created.

## 33.3 Project Agent Setup

Button:

```text
Generate Project Agent Token
```

Returns:

- token.
- Hub MCP config.
- client-specific setup.
- copyable start prompt.
- expiration/rotate controls.

## 33.4 Logging

Token values must not appear in:

- application logs.
- audit payload.
- Worker reports.
- generated troubleshooting bundle.

---

# 34. Resource and Context Budgets

## 34.1 Worker RAM Targets

Targets, not guarantees for every language server:

```text
Idle Worker:                     <= 100MB RSS
Normal graph query:              <= 200MB RSS
Incremental indexing:            target <= 400MB RSS
One external semantic/LSP task:  one process at a time
Runtime collector:               isolated, timeout/cap
Heavy concurrent jobs:           default 1
```

No always-on:

- Memgraph.
- Qdrant.
- local LLM.
- all-project LSP servers.
- runtime profilers.

## 34.2 Disk Defaults per Worker Host

```yaml
storage:
  global_soft_mb: 2048
  global_hard_mb: 3072
  minimum_free_disk_mb: 8192
  per_project_soft_mb: 768
  per_project_hard_mb: 1536
  raw_runtime_global_mb: 512
  content_cache_global_mb: 512
```

All values are configurable. Very large projects may override them, but consuming 18 GB by default is unacceptable.

## 34.3 Cleanup Order

1. tmp.
2. expired raw traces.
3. completed ChangeRun content cache beyond retention.
4. optional semantic embeddings.
5. external LSP caches.
6. inactive project indexes by LRU.
7. normalized old runtime detail according to retention.

Never delete:

- source.
- project manifest/policy.
- Hub quality history.
- canonical Wiki knowledge.

## 34.4 Context Cost of Shadow Workspace

A shadow copy does not enter LLM context merely by existing. Token usage increases only when agents read more information.

Prevent context duplication:

- Place the workspace outside the primary source root.
- Open only one active root in the Worker/agent workflow.
- shadow path excluded from index of production instance.
- Tool responses never return duplicate copies of a symbol.

## 34.5 Resource Cost of Optional Workspace Modes

| Mode | Disk | RAM | Git/GitHub dependency |
|---|---:|---:|---|
| `live` | Almost no additional storage | Almost none | none |
| `reflink` | Small initially; grows with changed blocks | Negligible | none; filesystem support required |
| `rsync` | Up to filtered source size | Temporary, low | none |
| `local_git_worktree` | Working-tree copy; shared Git objects | Negligible | local Git only; no remote/GitHub dependency |
| release directory | Copy of release files | Negligible | none |

When enabled, workspace storage is separate from the derived-graph cap and displayed independently in the UI so users understand its cost.

---

# 35. Optional Safe Workspace Capability

This capability remains optional and must not distract from the product's purpose.

## 35.1 Auto Selection

`workspace.mode: auto_copy` selects in order:

1. Reflink when supported.
2. rsync filtered copy.
3. A local Git worktree when local Git is available and suitable.
4. fallback to live with warning.

## 35.2 No GitHub Dependence

Local Git worktrees require no remote. Changing a remote or deleting GitHub repositories does not affect them while local `.git` exists. Without local Git, select another method.

## 35.3 Scope

This capability does not require VPS snapshots, database rollback or deployment orchestration. It only supports editing and checking a source copy before copying changes back when project policy chooses that workflow.

---

# 36. Manual Snapshot and External Restore

## 36.1 Manual Snapshot Marker

The UI/agent may optionally store:

```text
manual_snapshot_reference
created_at_declared
notes
```

This is a declaration only. There is no VPS-provider verification or default gate requirement.

## 36.2 Restore Detection

When the source revision changes abruptly to an earlier/known revision or a sequence inconsistent with the last current state:

- create `RecoveryEvent`.
- mark reports after restored revision historical/stale.
- rebuild graph from current source.
- show Hub banner: `External restore/revert detected or suspected`.
- do not merge current evidence with later abandoned revision.

## 36.3 Snapshot Restore Acceptance

Test a full host-snapshot restore that also reverts the local Worker database, and a source-only revert while Hub history remains ahead.

---

# 37. Central Hub API

Prefix:

```text
/api/v1
```

## 37.1 Auth and Tokens

```http
POST   /auth/login
POST   /auth/re-auth
GET    /tokens
POST   /tokens
POST   /tokens/{id}/reveal
POST   /tokens/{id}/rotate
DELETE /tokens/{id}
POST   /tokens/{id}/test
```

## 37.2 Downloads and Bootstrap

```http
GET  /downloads
GET  /downloads/{platform}/{artifact}
POST /worker-enrollment-tokens
GET  /agent-bootstrap
GET  /docs/{path}
GET  /docs/{path}?format=raw
GET  /docs-bundle.zip
```

## 37.3 Projects

```http
POST   /projects
GET    /projects
GET    /projects/{project_id}
PATCH  /projects/{project_id}
POST   /projects/{project_id}/recover-token
POST   /projects/{project_id}/retire
```

## 37.4 Instances and Workers

```http
POST /workers/enroll
POST /workers/{worker_id}/heartbeat
GET  /workers
GET  /workers/{worker_id}
POST /projects/{project_id}/instances/register
GET  /projects/{project_id}/instances
POST /instances/{instance_id}/retire
```

## 37.5 Revisions and Change Runs

```http
POST /instances/{instance_id}/revisions
GET  /projects/{project_id}/revisions
POST /instances/{instance_id}/change-runs
PATCH /change-runs/{change_run_id}
GET  /projects/{project_id}/change-runs
GET  /change-runs/{change_run_id}
```

## 37.6 Reports and Health

```http
POST /instances/{instance_id}/reports/index
POST /instances/{instance_id}/reports/review
POST /instances/{instance_id}/reports/tests
POST /instances/{instance_id}/reports/smoke
POST /instances/{instance_id}/reports/runtime
POST /instances/{instance_id}/reports/dataflow
POST /instances/{instance_id}/reports/quality
GET  /projects/{project_id}/health
GET  /projects/{project_id}/quality-history
GET  /projects/{project_id}/findings
```

## 37.7 Context and Agent Setup

```http
GET /projects/{project_id}/context-pack
GET /projects/{project_id}/agent-context
GET /projects/{project_id}/agent-setup/{client}
GET /projects/{project_id}/architecture-summary
```

## 37.8 Remote Analysis

```http
POST /projects/{project_id}/remote-analysis
GET  /remote-analysis/{request_id}
POST /workers/{worker_id}/remote-analysis/{request_id}/claim
POST /remote-analysis/{request_id}/result
```

## 37.9 Wiki Binding

```http
GET    /projects/{project_id}/wiki-binding
PUT    /projects/{project_id}/wiki-binding
DELETE /projects/{project_id}/wiki-binding
POST   /projects/{project_id}/wiki-update-pending
```

## 37.10 Storage and Metrics

```http
GET /workers/{worker_id}/resources
GET /instances/{instance_id}/storage
GET /projects/{project_id}/token-metrics
GET /system/health
```

---

# 38. Hub Database Model

Recommended tables:

```text
owner_users
agent_credentials
worker_hosts
workers
projects
project_aliases
project_instances
source_revisions
index_snapshots
change_runs
analysis_runs
evidence
findings
test_runs
smoke_runs
runtime_runs
dataflow_runs
quality_runs
project_context_packs
wiki_bindings
remote_analysis_requests
remote_analysis_results
recovery_events
agent_policy_versions
release_artifacts
audit_events
job_queue
```

Raw source code must not appear as a column or blob in the Hub schema.

SQLite WAL is the default for a small single-node Hub. Provide a migration-compatible repository layer for future PostgreSQL support; Redis is not required.

---

# 39. Minimal Operational Security

The following are mandatory without turning the product into a separate security project:

- TLS for the Hub.
- A single owner account; optional TOTP.
- encrypted token vault.
- project-scoped/revocable credentials.
- Worker outbound only.
- Prefer stdio for local MCP; HTTP binds only to localhost.
- no unauthenticated report/docs-with-secrets endpoints.
- no full source in Hub.
- bounded/redacted command output.
- No secrets in Git, reports or logs.
- Audit token creation/reveal/rotation/revocation and remote analysis.

Explicit non-goals:

- Agent OS sandboxing.
- Confining agents to project directories.
- Managing root privileges.
- Scanning every build script as hostile code.
- A comprehensive threat model as part of the core specification.

When using external code, preserve attribution/notices required by its license. SBOM/license auditing need not become a quality gate for users' projects.

---

# 40. Hub and Worker Deployment

## 40.1 Hub Components

```text
codeintel-hub-api
codeintel-hub-ui
hub database
reverse proxy/TLS
artifact/download storage
```

Docker Compose deployment is permitted and recommended for compatibility with the existing knowledge hub.

## 40.2 Worker Components

```text
single Worker binary/service
CLI
local MCP
client adapters/skills
derived local data
```

systemd:

- restart policy.
- resource limits configurable.
- log rotation.
- health endpoint/CLI.

## 40.3 Hub Availability

Hub outage:

- Local analysis continues.
- Local MCP continues.
- ChangeRuns/reports remain in the local spool.
- Uncached remote-agent queries are unavailable.
- Synchronization resumes automatically.

## 40.4 Worker Availability

Worker offline:

- The Hub presents last-known context only.
- Remote live analysis is unavailable.
- No false Current status.

---

# 41. Observability and Feedback

## 41.1 Worker Metrics

- idle/current RSS.
- peak index RAM.
- query latency.
- index duration/files changed.
- graph DB size.
- content cache size.
- semantic/LSP duration/peak.
- test/smoke/runtime duration.
- trace size.
- Hub sync errors.
- lock/change-run state.

## 41.2 Quality Feedback

The Hub lets owners mark:

```text
Gate result was accurate
False PASS — actual project was broken
False FAIL
Finding useful/not useful
```

Feedback is retained for benchmark/risk tuning without rewriting history retrospectively.

## 41.3 Token Metrics

When clients support usage export:

- input tokens.
- output tokens.
- tool calls.
- file reads/grep calls.
- context bytes returned.
- change/review cycles.
- time to completion.

When clients provide no token counts, store proxy metrics rather than inventing exact numbers.

---

# 42. Failure Semantics

```text
Graph stale              → STALE, refresh required
Graph corrupt            → INVALID, rebuild; never answer Current
Source changes mid-run   → evidence invalid, UNKNOWN
LSP/semantic unavailable → PARTIAL/UNKNOWN if required
Runtime unavailable      → UNKNOWN if required
Smoke endpoint fails     → FAIL if it passed baseline/required
Hub unavailable          → local works, sync pending
Worker offline           → Hub last-known only
Disk low                 → prune derived data; block heavy trace, not source work
Lock stale               → recover after lease timeout and audit
Unsupported language     → explicit coverage gap, no false exact result
Snapshot restore         → RecoveryEvent, rebuild and stale later evidence
Token revoked            → connection fails clearly, no silent fallback
```

---

# 43. Implementation sequence — every stage is required for production delivery

These stages are recommended implementation order, not independently deliverable products.

## Gate A — Foundation

- monorepo.
- Hub UI/API/DB.
- Worker service/CLI.
- installer/enrollment.
- Git-independent project/instance identity.
- SourceRevision.
- Context Pack skeleton.
- docs/token center skeleton.

## Gate B — Structural Intelligence

- parser.
- SQLite graph.
- incremental index.
- exact/FTS/graph retrieval.
- framework foundations.
- local MCP.
- token-bounded outputs.

## Gate C — Change Quality

- ChangeRun.
- non-Git diff/content cache.
- risk engine.
- blast radius.
- review engine.
- tests/commands.
- HTTP smoke.
- Quality Gate.
- unverified-change detection.

## Gate D — Deep Intelligence

- semantic resolver for priority languages.
- framework adapters.
- runtime adapters/trace ingestion.
- data-flow.
- dead-code candidates.

## Gate E — Central and Remote Workflow

- remote Hub MCP.
- live analysis relay.
- full Context Pack.
- Wiki integration.
- complete Agent Center.
- downloads/install/update.
- reveal/copy/rotate tokens.

## Gate F — Production Hardening

- failure injection.
- resource caps/cleanup.
- migrations/upgrade/rollback for product.
- benchmarks with Codex CLI.
- real pilot validation.
- documentation and handoff.

Do not declare production readiness before Gate F.


---

# 44. Mandatory acceptance tests

## 44.1 Identity and Portability

1. A project without Git registers, is analyzed and receives a quality gate.
2. Changing a Git remote does not create a new project.
3. Deleting a Git remote does not break the system.
4. Moving a project to another VPS with the same manifest creates a new instance under the same history.
5. Different revisions of the same project on two hosts do not merge evidence.
6. `codeintel fork-project` creates a new identity with optional parent lineage.
7. A lost manifest is restored using a rebind token, not a Git URL alone.

## 44.2 SourceRevision and Indexing

8. Dirty source changes SourceRevision even without a commit.
9. Ignored log/cache/upload changes do not change the revision.
10. Relevant source-file changes do change the revision.
11. A file written during indexing triggers retry/debounce; a partially read graph is never published.
12. A mid-index Worker crash preserves a valid previous published graph or marks it invalid; never mark a partial graph Current.
13. Corrupt SQLite databases are detected and rebuilt.
14. Index rebuilding consumes CPU/I/O only and requires no LLM/API.

## 44.3 Graph Safety

15. Empty caller results include `absence_proven=false` unless the analyzer proves complete scope.
16. Heuristic edges are never presented as exact.
17. Unsupported language areas produce coverage gaps.
18. The agent workflow requires reading actual source before significant edits.
19. Stale graphs cannot support PASS.

## 44.4 ChangeRun and Concurrency

20. `preflight_change` creates a before revision and ChangeRun.
21. Post-change diffs work without Git.
22. External changes during review invalidate old evidence.
23. Another agent sees the instance as in-use without damaging its active ChangeRun.
24. Read-only queries keep working during index publication.
25. Source changes without a quality gate appear as UNVERIFIED_CHANGE on the main dashboard.

## 44.5 Risk and Review

26. Small CSS changes do not trigger heavy runtime analysis without justification.
27. Shared-utility changes receive increased blast-radius analysis.
28. Route/auth/database changes receive appropriate required checks.
29. Public-signature changes with unadapted callers create blocking findings.
30. Newly unresolved imports/references block PASS.
31. Test gaps remain visible and do not automatically become “no tests needed.”
32. Review bundles do not contain the whole repository.

## 44.6 Tests and Smoke

33. Baselines identify existing failing tests without wrongly attributing them to the change.
34. New failing tests produce FAIL.
35. Endpoints passing before and failing afterward produce FAIL.
36. Dynamic page bodies do not fail solely because of body hashes when hashing is disabled.
37. Service/CLI smoke checks retain their revision and command.
38. High-risk changes missing required behavioral evidence produce UNKNOWN.

## 44.7 Runtime and Data-flow

39. Observed runtime edges are retained as RUNTIME_OBSERVED with workload/revision.
40. Unobserved paths are never marked nonexistent.
41. Old runtime traces are shown as STALE against the current revision.
42. Trace timeouts clean up without leaving collectors active.
43. Unsupported dynamic areas produce UNKNOWN in data-flow analysis.
44. `NO_FLOW` is permitted only in fixtures with declared complete scope.

## 44.8 Dead Code

45. An obviously unused internal function appears as a candidate.
46. Route callbacks are not classified as high-confidence dead code.
47. Configuration/reflection/event registration lowers confidence.
48. No code path automatically deletes source.
49. Removal workflows require review/tests/gating after deletion.

## 44.9 Hub, Remote Agent and Wiki

50. The Hub receives neither full source nor raw graphs by default.
51. Remote agents receive context packs and code health without source.
52. An online Worker serves live structured queries through outbound relay.
53. An offline Worker returns last-known context with clear STALE labeling.
54. An offline Hub does not block local MCP.
55. Reports synchronize after Hub recovery.
56. Wiki none/local/remote modes work.
57. Non-code Wikis without the capability receive no Code Intelligence prompt.
58. Wiki failure does not block quality gates.
59. Durable updates are written only after validated changes and only when durable knowledge exists.

## 44.10 Installer, Docs and Tokens

60. Installation is idempotent and preserves existing client settings.
61. Codex/Claude/Hermes adapters are installed and checked.
62. Every Markdown document is available rendered, raw and as a download.
63. Project-specific bootstrap returns the correct configuration/prompt.
64. Worker enrollment tokens are short-lived and single-use.
65. Project Agent tokens support reveal/copy/rotate/revoke.
66. Revoked tokens stop working.
67. Token values never appear in logs.
68. Version mismatches appear in the UI.

## 44.11 Resources and Recovery

69. Idle/index/query/runtime RAM is measured for every release.
70. Global disk caps are enforced without deleting source/manifests/history.
71. LRU eviction of inactive graphs allows subsequent rebuilding.
72. Low disk space prevents heavy traces and produces a warning.
73. Manual VPS snapshot restores create a RecoveryEvent and invalidate the currentness of later evidence.
74. Optional reflink/rsync workspaces are not indexed twice.
75. Live mode works fully without shadow workspaces.

## 44.12 Quality Gate False-confidence Fixtures

76. stale tests never yield Current PASS.
77. stale runtime never yields Current PASS.
78. unsupported analyzer never silently yields PASS.
79. graph empty never means safe deletion.
80. source drift never yields PASS.
81. smoke regression overrides otherwise-clean structural review.
82. blocking UNKNOWN remains visible in final chat report.

---

# 45. Benchmark Program — Codex CLI Alone vs Codex + Platform

Do not rely on reference repositories' marketing figures. Benchmark this implementation.

## 45.1 Dataset

- 20–30 real tasks.
- At least three projects.
- A small project.
- A medium/large legacy project.
- A project with dynamic/runtime behavior.
- A mix of bug fixes, features, refactors, routes/APIs, databases, CSS and configuration.

## 45.2 Arms

```text
A. Codex CLI alone
B. Same Codex/model/settings + Code Intelligence Platform
```

Use the same prompt, initial revision and, where possible, test environment.

## 45.3 Metrics

Quality:

- task correctness.
- regressions.
- missed dependencies.
- review findings caught before completion.
- false PASS.
- appropriate UNKNOWN.
- rework.

Efficiency:

- total input/output tokens.
- tool calls.
- file reads/grep/search.
- context bytes returned.
- wall time.
- CPU/RAM/disk.

## 45.4 Weight

```text
Quality/correctness       60%
False-confidence safety   15%
Regression prevention     10%
Token efficiency           7%
Resource use               4%
Latency                    4%
```

## 45.5 Production Targets

Hard targets:

- no increase in false PASS.
- no quality regression versus baseline.
- no unbounded MCP response.
- overall median tokens not worse than baseline across full task set.

Efficiency targets:

- medium/large understanding/review tasks: target at least 25% median token reduction.
- targeted retrieval tasks: target at least 30% fewer raw file reads/grep calls.
- trivial tasks: added token overhead target <=15%.

When quality improves but deep tasks use more tokens, report `quality-per-token` rather than concealing the cost.

## 45.6 Hub Benchmark View

The Hub displays per-task and aggregate:

```text
baseline tokens
tool-assisted tokens
files read
tool calls
defects found
false/true gate outcome
resource cost
```

---

# 46. Production Deliverables

At handoff:

- The product source repository.
- tagged release build.
- Hub Docker Compose/deployment config.
- Worker binaries required platforms.
- install/uninstall/update scripts.
- systemd units.
- DB migrations.
- OpenAPI docs.
- Hub MCP schema/docs.
- Worker MCP schema/docs.
- Codex/Claude/Hermes adapters.
- Skills and raw Markdown docs.
- sample projects/manifests/policies.
- unit/integration/end-to-end tests.
- all Acceptance fixtures.
- failure-injection runner.
- benchmark runner/results.
- backup/restore guide for Hub.
- product upgrade/rollback guide.
- Project migration/recovery guide.
- Knowledge Hub integration patch/spec.
- release checksums.
- third-party notices for copied/reused code.
- architecture decision log.
- changelog.

---

# 47. Implementation contract — changes require specification approval

1. Source code is the source of truth.
2. Project identity is not a Git URL.
3. Git/GitHub are optional.
4. Graph derived/rebuildable.
5. By default, the Hub retains no full source.
6. Worker analysis runs beside the code.
7. Quality results are bound to source revisions.
8. UNKNOWN is a valid result.
9. Empty graph results do not prove absence.
10. Dead code is never deleted automatically.
11. Core features require no remote LLM API.
12. Risk routing is automatic and as deterministic as practical.
13. Runtime observation does not prove absence.
14. Hub outages do not block local work.
15. Offline Workers are not presented as Current in the Hub.
16. This product does not confine agents to project paths.
17. Snapshots remain manual and external.
18. Shadow workspaces are optional.
19. The MCP surface is compact and token-bounded.
20. Documentation, installers and token management are required product features.
21. Remote agents receive context packs and may request structured live analysis.
22. Do not declare production readiness before every gate and acceptance test passes.

---

# 48. Canonical Agent Operating Policy

Make the following text available in the Hub, raw Markdown, MCP initialization and client-specific skills:

```text
CODE INTELLIGENCE OPERATING POLICY

1. When working on a code project, detect and open its local Code Intelligence project.
2. Use the central Hub/Wiki for durable context and history; use the local Worker and actual source for current code truth.
3. Treat graph results as discovery evidence, not canonical truth.
4. Absence from a graph is not evidence of absence from the application.
5. Before modifying a relevant component, inspect the actual current source.
6. Before a significant change, run preflight_change and follow required_checks.
7. Make edits using your normal shell and file tools. Code Intelligence does not replace implementation tools.
8. After edits, refresh the source revision and run review_change.
9. Run verify_change and all checks required by the risk engine.
10. Runtime observations prove only what the declared workload observed.
11. Dead-code results are candidates only; never delete solely because callers=0.
12. Evidence is current only for the exact source_revision_id on which it was produced.
13. On FAIL, repair and repeat. On blocking UNKNOWN, gather evidence or report the unresolved uncertainty.
14. Do not represent a task as complete until quality_gate has run for the current source revision.
15. Update the Wiki only with durable, validated conclusions; do not copy raw graph relationships into canonical knowledge.
16. Before ending the session, report the Quality Gate, revision, checks performed, and remaining warnings/unknowns in the chat.
```

---

# 49. Knowledge Hub Conditional Prompt Block

```text
CODE INTELLIGENCE CAPABILITY

This Wiki is linked to a Code Intelligence project.

Use the Wiki for durable project knowledge, business and technical rationale,
decisions, invariants, operational context, and validated long-term conclusions.

Use Code Intelligence and the actual current source for live code structure,
callers, dependencies, impact, review, tests, runtime observations, data-flow,
and current code-quality evidence.

The central Code Intelligence Context Pack may answer project status, history,
architecture summaries, known risks, and last-known health without storing the
full source. If an exact current technical answer is needed and an active Worker
is available, request live analysis through the Code Intelligence Hub.

Do not treat a derived graph as canonical truth. Do not infer absence from an
empty graph result. Do not copy raw graph dumps or volatile caller lists into
canonical Wiki pages.

After a validated change, record only durable consequences worth remembering,
including date, source revision, quality result, material warnings/unknowns,
and the stable architectural or behavioural consequence.
```

---

# 50. Copyable Start Working Prompt

```text
This project is managed by Code Intelligence.

Before changing code:
- detect/open the local project;
- load the short project context and linked Wiki if configured;
- refresh stale indexes;
- run preflight_change for the requested task;
- use graph/semantic results for discovery, then inspect the actual relevant source.

After changing code:
- refresh the current source revision;
- run review_change;
- run verify_change and every check required by the risk engine;
- obtain quality_gate for the current revision;
- repair and repeat on FAIL or blocking UNKNOWN.

Do not infer that a caller, dependency or flow does not exist merely because a
graph query returned no result. Runtime proves only the workload that ran.
Dead-code findings are candidates, not automatic deletion instructions.

At the end, state in this chat:
- Quality Gate result;
- source revision;
- what changed;
- what was checked;
- tests/smoke/runtime performed;
- remaining warnings or unknowns;
- whether a durable Wiki update was made or was unnecessary.
```

---

# 51. Programmer Handoff Checklist

- [ ] Read the entire document.
- [ ] Do not bind project identity to Git/GitHub/path/host.
- [ ] Support diffs and review without Git.
- [ ] Keep source/graph analysis local.
- [ ] Build Hub context packs without full source.
- [ ] Separate local Worker MCP from remote Hub MCP.
- [ ] Add structured live analysis relay through outbound Worker connections.
- [ ] Build source revisions and ignore rules before quality gates.
- [ ] Build invariant tests before relying on analyzers.
- [ ] Add HTTP smoke checks before heavy runtime tracing.
- [ ] Add semantic resolution with provenance and ambiguity.
- [ ] Add deterministic review/risk and required checks.
- [ ] Never use unsupported/stale evidence to award PASS.
- [ ] Do not add automatic dead-code deletion.
- [ ] Do not add a remote LLM dependency to the core.
- [ ] Do not keep Memgraph/Qdrant/LSP/runtime running continuously.
- [ ] Keep idle Worker usage and storage caps within targets.
- [ ] Do not require shadow workspaces or snapshots.
- [ ] Agents remain free to work outside project roots.
- [ ] Provide complete Agent Center, Downloads, API/MCP, Docs and Tokens interfaces.
- [ ] Owners can reveal/copy/rotate/revoke tokens.
- [ ] All documentation is available rendered and as raw Markdown.
- [ ] The installer configures Codex/Claude/Hermes idempotently.
- [ ] Add a final chat-report policy.
- [ ] Run every acceptance test and the benchmark before declaring production readiness.

---

# 52. Definition of Done

The product is complete only when:

1. Agents can enter a real project and complete the workflow without users choosing tools or verification depth.
2. Code understanding, review and runtime evidence measurably improve work quality.
3. Quality gates award no false PASS in mandatory fixtures.
4. The system works without GitHub or local Git.
5. Moving projects between VPS hosts preserves identity/history and rebuilds locally.
6. Remote agents receive context packs and request live analysis when Workers are online.
7. The Hub retains no full source.
8. Wiki none/local/remote modes work without duplicating knowledge.
9. Codex/Claude/Hermes setup is available through the UI and raw Markdown links.
10. The Hub provides a simple Worker installation link/command.
11. The UI supports token copy, reveal, rotation and revocation.
12. RAM, disk and token budgets are measured and meet targets, or justified exceptions are documented.
13. Every acceptance test, failure-injection check and benchmark passes.
14. The product itself has upgrade, rollback and backup paths.
15. Users receive a clear dashboard showing Current/STALE/UNVERIFIED, dates, revisions, checks and unknowns.

---

# 53. Reference Implementations — Capability Sources

Reference only; final product owns its contracts and does not require all repositories at runtime:

- https://github.com/colbymchenry/codegraph
- https://github.com/DeusData/codebase-memory-mcp
- https://github.com/tirth8205/code-review-graph
- https://github.com/vitali87/code-graph-rag

Benchmark this implementation's capabilities. Never copy an external benchmark claim as production proof.

---

# 54. Final Product Statement

Users should be able to tell Codex/Hermes/Claude:

> “Make this change.”

Without requiring technical decisions from the user, the system lets the agent perform:

```text
detect project
→ retrieve short durable context
→ verify current source/index
→ understand relevant code efficiently
→ preflight risk and blast radius
→ read actual source
→ implement normally
→ review the delta
→ run targeted tests/smoke/runtime according to risk
→ quality gate on the exact current revision
→ repair and repeat
→ report clearly in chat and Hub
→ preserve durable knowledge
```

This is the practical definition of Code Intelligence Platform for the operating environment described above.


# Implementation status

Updated: 2026-09-09. Canonical inputs: the Final Production Spec V1,
`IMPLEMENTATION_DECISIONS_V1.md`, `POST_IMPLEMENTATION_HARDENING_ADDENDUM.md` and
`SIMPLE_ACCESS_AND_PORTABILITY_ADDENDUM_V1_2.md`.

The V1.2 simple-access addendum is implemented as a compatibility layer: Host Install is one-time,
Host Access controls only create/link, one Project Access serves authorized Agents, and all code
intelligence remains local-first and Hub-optional.

No build or deployment is a Production Readiness declaration. That declaration remains blocked until
all 82 parent acceptance tests, all 35 implementation-lock tests, failure injection, three-project pilots,
and the 20–30 task Codex A/B benchmark have completed with recorded results.

| Parent area | Implementation module | Status | Automated evidence | Remaining blocker |
|---|---|---|---|---|
| 6, 8, 27–30, 37–40 Hub/domain | `hub/app`, Alembic migration, Jinja/HTMX UI | Implemented | Hub API integration suite | Pilot and failure injection |
| 7, 10 Source truth/revision | `codeintel-core` | Implemented | non-Git revision, ignore, stale-evidence tests | large-project resource benchmark |
| 9 Project identity/portability | core, Worker registry, Hub registration/recovery | Implemented | same-manifest/new-instance and restore tests | real cross-host pilot |
| 12 Graph store | `codeintel-graph` | Implemented | WAL, failed-publication rollback, corrupt-derived-DB replacement and heuristic provenance tests | process-kill pilot during large index |
| 13 Parsers/semantic/frameworks | `codeintel-indexer`, `codeintel-semantic`, `codeintel-frameworks` | Implemented baseline | import/type-aware resolver, ambiguity/dynamic-boundary tests, seven priority adapter detection and route fixtures | lazy external LSP escalation and broader real-project fixtures |
| 14 Retrieval/context limits | graph, Worker RPC, MCP adapters | Implemented baseline | response bound and eleven-tool surface tests | tokenizer-specific client measurements |
| 15–17 Local MCP/workflow | `codeintel-mcp`, CLI, canonical policy | Implemented | MCP tests and generated docs checks | live client E2E for all three clients |
| 18 Concurrency/drift | Worker advisory locks/watcher | Implemented baseline | lock/source revision logic | multi-process and watcher failure injection |
| 19 Risk engine | `codeintel-core` | Implemented baseline | deterministic risk tests | benchmark-based tuning |
| 20 Review | `codeintel-review` | Implemented baseline | bounded non-Git diff tests | framework-specific review fixtures |
| 21–22 Commands/smoke | `codeintel-quality` | Implemented | bounded/redacted runner tests | service/systemd/Docker scenario fixtures |
| 23 Runtime | `codeintel-runtime`, quality router, graph runtime overlay | Implemented baseline | JSON/NDJSON/OTLP HTTP normalization, NOT_OBSERVED, aggregation, timeout cleanup and Python/Node/PHP plugin contracts | real collector binaries and workload pilots for all priority runtimes |
| 24 Data-flow | deterministic flow/call/route/current-runtime reachability | Implemented baseline | structural-edge exclusion, FOUND path and declared-complete UNKNOWN/NO_FLOW tests | language-aware value propagation and real-project fixtures |
| 25 Dead code | graph candidate audit | Implemented baseline | no auto-delete design | route/config/reflection confidence fixtures |
| 26 Quality Gate | Worker and Hub deterministic mirrors | Implemented | false-confidence unit/API tests | complete acceptance fixture matrix |
| 27–29 Context/Wiki/remote relay | Hub Context Pack, WikiConnector, long-poll jobs | Implemented baseline | Wiki-none/API and idempotent models | local/remote Wiki adapters and live relay pilot |
| 31–33 Docs/install/tokens | Hub UI, docs, signed atomic installer/updater, AES-GCM vault | Implemented | CSRF/token/revoke/audit/docs tests; live amd64 idempotency/identity-preservation run | clean-host amd64 and arm64 installer pilots |
| Hardening onboarding | opt-in Skill, concurrent init lock, automatic Hub upsert, durable offline retry, Wiki state | Implemented | online idempotent Hub tests; process-level offline/restart/reconnect fixture | second physical Host pilot |
| Simple access/local portability | Host Install + Host Access + one Project Access, local binding, portable Project Skill/context | Implemented | Hub cross-project/rotation/revoke test; Rust portability and filesystem permission tests | clean second-Host and clone/fork pilot |
| Agent handoff UX | universal Prompt, per-project Project Access setup, Hebrew simple guides, plaintext-on-demand access guide, curl + synchronized Complete Agent Kit | Implemented | project scoping/rotation and UI/docs tests; signed release bundle verification | nontechnical owner pilot |
| On-demand visualization | bounded local graph relay + ephemeral Canvas 3D UI | Implemented | source-field rejection, bounds, expiry/consume-once and exact Worker payload tests | browser/device pilot on large real graph |
| Hub list scalability | project search and server-side 25-row UI pagination | Implemented | search/pagination integration test | large live catalog pilot |
| 34 Resources | caps/config/on-demand tasks | Implemented policy | build metrics available | release RAM/disk measurements |
| 35 Shadow workspace | live mode complete | Partial optional capability | live default tests | reflink/rsync modes not yet implemented |
| 36 Restore | RecoveryEvent/currentness reset | Implemented | Hub restore transition test | full VPS snapshot fixture |
| 44 acceptance | `fixtures`, failure-injection runner, Rust/Hub/benchmark-harness tests | In progress | no-Git/Wiki fixtures; atomic publication/corruption/runtime-timeout/spool scenarios | 82 + 35 complete mapping and remaining failure scenarios |
| 45 benchmark | `benchmarks/benchmark.py` | Harness implemented; production run not started | schema validation and no-false-ready unit tests | 20–30 real Codex A/B tasks on at least three real projects |
| 46 deliverables | monorepo/release/deploy/docs | In progress | signed artifact build | tagged release after acceptance |

## Validation deployment

- Hub: `https://code-hub.example.com` behind Nginx and Let's Encrypt TLS.
- Deployed version: Hub and local Worker `1.3.0`; the onboarding and simple-access migrations were exercised against
  the live SQLite database, including safe recovery from a deliberately observed partial DDL retry. Host Access is separated
  from the internal Worker credential, and every registered Project receives one shared Project Access credential.
- Hub container: healthy, read-only filesystem, root-only host vault key copied to private tmpfs,
  then the application runs as unprivileged UID 10001 with zero effective capabilities.
- Worker: enrolled through the public signed installer, running under systemd, local MCP registered
  with Codex, and the monorepo registered with `wiki.mode=none`.
- Remote relay: live `project_status` request completed through outbound long-poll with `CURRENT`
  freshness and no source body.
- Backup: encrypted online SQLite backup and DB+master-key restore were exercised successfully.
- Current automated suites: 72 Rust tests, 18 Hub tests and 3 benchmark-harness tests pass; Rust formatting, Clippy, Ruff, ShellCheck, HTTP smoke,
  artifact checksums and Ed25519 release signature pass.
- The live amd64 installer and updater preserve Worker identity/credential, pin the release key,
  wait for the local socket, and keep a rollback binary. The exact-revision Gate is rerun after each
  hardening change; no prior result is promoted to the current revision and no failed check is hidden.

## Graph-node collision regression — 1.2.1

- Root cause: v1.2.0 derived a Symbol primary key from `kind + qualified_name`, while some
  declarations (notably repeated PHP functions separated by namespace blocks) did not carry a
  namespace/range discriminator into that qualified name.
- Fix: v1.2.1 uses a full SHA-256 of a versioned, length-delimited canonical identity containing
  logical root, relative path, language, kind, qualified/container identity and source range.
  Same canonical entities merge provenance explicitly; non-equivalent candidates are rejected with
  `NODE_ID_COLLISION` diagnostics and cannot be published.
- Evidence: PHP namespace fixture, file/class/root/kind/range collision matrix, explicit provenance
  merge and diagnostic tests, deterministic retry/rebuild, and failed-publication preservation all
  pass. Local Worker update from 1.2.0 to 1.2.1 preserved its registered Project/Instance and rebuilt
  the graph as CURRENT.
- External Drill Center recovery remains deliberately unclaimed until its existing Host runs
  `codeintel-update --hub https://code-hub.example.com` followed by `codeintel open .`; this needs
  no re-enrollment, no `init`, and no Project/Wiki/token replacement.

## Gate summary

- Gate A: implemented and locally tested.
- Gate B: implemented baseline with hybrid semantic resolution and framework registry; real-project depth validation remains.
- Gate C: implemented baseline; complete fixtures remain.
- Gate D: partial, therefore not passed.
- Gate E: implemented baseline; real multi-host pilot remains.
- Gate F: not passed.

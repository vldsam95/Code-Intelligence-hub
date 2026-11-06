# Implementation Architecture

## Hub

Python 3.12+, FastAPI, SQLAlchemy 2, Alembic, SQLite WAL, Jinja2, HTMX and minimal JavaScript. The job queue is stored in the database; Redis is not required. Nginx provides TLS and security headers.

## Worker

A stable Rust workspace. The Worker daemon, CLI and MCP adapter share the core and SQLite graph. Local control uses a Unix socket; agent MCP uses stdio. At most one heavy job and one lazily started LSP process run at a time; runtime analysis is on demand.

The Worker is organized into independent contracts rather than a monolithic indexer:

- `codeintel-core` — Identity, non-Git revisions, policy, risk and gate invariants.
- `codeintel-graph` — SQLite WAL, atomic publication, FTS and edges carrying provenance.
- `codeintel-indexer` — Tree-sitter orchestration and snapshot publication.
- `codeintel-semantic` — Hybrid local resolution, imports/types, ambiguity and dynamic boundaries.
- `codeintel-frameworks` — A registry for Laravel, WordPress, Express, Next.js, FastAPI, Django and Docker Compose.
- `codeintel-runtime` — On-demand JSON/NDJSON/OTLP normalization, HTTP traces, command wrapping and language-plugin contracts.
- `codeintel-review` / `codeintel-quality` — delta review, checks, smoke, runtime/data-flow,
  authorized local-only security scenarios and quality gates.

The semantic resolver returns `COMPLETE` only for a supported analyzer run. Unresolved or ambiguous
call sites are retained separately and checked against the changed-file scope. A healthy analyzer status
therefore does not turn an unknown in a changed file into a PASS.

A call without a local declaration is classified as `EXTERNAL_OR_UNINDEXED`. It is neither speculatively
linked to another symbol nor used to prove absence. It is a warning requiring corroboration when relevant to a security flow;
parser failures, unresolved internal targets and ambiguity within the changed scope remain blockers.

The embedded indexer also supports C# and Rust through Tree-sitter. Declarations,
calls and type annotations use the same revision-aware semantic contract. Unproven resolution
remains unresolved/ambiguous; language-toolchain builds and runtime checks are separate evidence required according to risk.

Security correlation uses an exact `route_symbol` to bind an HTTP/cost result to one route
and a bounded downstream graph. It does not claim full graph coverage or return source bodies, and returns UNKNOWN
when a route is missing or ambiguous. Global semantic gaps outside changed files are warnings; a gap within the changed
scope remains a blocker.

Data-flow analysis uses only `FLOWS_TO`, call/route/HTTP/async and revision-compatible runtime relationships.
Structural relationships such as `CONTAINS` and `IMPORTS` cannot establish a `FOUND` verdict on their own;
`NO_FLOW` requires both an explicitly complete scope and complete analysis coverage.

## Transport

The Worker initiates HTTPS heartbeats, spool synchronization and 25-second long polling. Live analysis is read-only and asynchronous. Workers expose no public inbound port.

## Verification assets

`fixtures/projects/` contains fixtures without Git or a Wiki. `benchmarks/benchmark.py` stores immutable
A/B results and fails the production benchmark until a real dataset of 20–30 tasks has complete
baseline/platform pairs. Missing token exports remain unknown rather than being presented as exact estimates.
Security datasets add ground-truth `required_findings` and recall/precision metrics for each arm.
Review bundles retain every changed path/hash but expose at most 12 excerpts of 800 characters plus
compact de-duplicated graph metadata. Omitted excerpts direct the Agent back to the actual source.

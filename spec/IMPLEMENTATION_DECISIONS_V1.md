# IMPLEMENTATION_DECISIONS_V1.md

## Code Intelligence Platform — Binding V1 Implementation Decisions

**Version:** 1.0
**Date:** 2026-08-17
**Status:** Binding companion to `CODE_INTELLIGENCE_FINAL_PRODUCTION_SPEC_V1.md`
**Purpose:** Lock implementation, production operations, security, backup, release and integration decisions intentionally left open in the main specification.

---

# 0. Relationship to the main specification and precedence

This document **does not replace, shorten or modify** the main specification:

`CODE_INTELLIGENCE_FINAL_PRODUCTION_SPEC_V1.md`

The main specification remains the **source of truth for product requirements, architecture, capabilities, quality semantics and acceptance tests**.

This document is the **V1 implementation lock**.

Precedence:

```text
1. An explicit requirement/invariant in the main specification
   ↓
   always takes precedence.

2. Where the main specification allows multiple valid implementations,
   ↓
   this document selects the binding V1 option.

3. If a genuine contradiction is discovered,
   ↓
   do not silently resolve it in code.
   Stop work on that part, document an ADR/issue and request specification review.
```

Developers and agents must not:

- Remove requirements to shorten development;
- Reclassify required capabilities as future work;
- Replace a core stack component without an ADR and specification approval;
- Declare production readiness before the gates and acceptance tests in the main document;
- Use this document to bypass graph distrust, source truth, `UNKNOWN`, quality gates or any other invariant.

---

# 1. Instructions for developers and AI programmers

Before writing production core code:

1. Read the **entire** main specification.
2. Read **this entire document**.
3. Create `IMPLEMENTATION_STATUS.md` mapping:
   - Parent Spec section;
   - Implementation module;
   - status;
   - tests;
   - unresolved blocker.
4. Create `ADR/` for exceptions only.
5. Do not reopen decisions locked here solely because of personal preferences.

The following statement is binding:

> **The main document is the binding source of truth. Requirements must not be removed to shorten development. Gates A–F define implementation order, not independently deliverable MVPs. Production readiness must not be declared before all applicable acceptance tests pass.**

---

# 2. Product principle — LLM Wiki is optional

Code Intelligence Platform must be a complete, useful product even when a project **has no LLM Wiki at all**.

Valid states:

```yaml
wiki:
  mode: none
```

```yaml
wiki:
  mode: local
  local_path: "./LLM-WIKI"
```

```yaml
wiki:
  mode: remote
  wiki_id: "..."
```

## 2.1 V1 decision

The default for `codeintel init` is:

```yaml
wiki:
  mode: none
```

unless the user/agent explicitly chooses otherwise.

`wiki.mode=none` must support the following without degrading core functionality:

- indexing;
- static graph;
- semantic/LSP resolution;
- preflight;
- risk;
- review;
- tests;
- smoke;
- runtime;
- data-flow;
- dead-code candidates;
- Quality Gate;
- Code Health;
- Project Context Pack;
- Hub history;
- local MCP;
- remote Hub MCP;
- live-analysis relay.

A Wiki outage, Wiki credentials or the absence of a Wiki must never block code quality.

## 2.2 Current development order

Initially:

- Build Code Intelligence Platform;
- Do not proactively modify the external LLM Wiki system's code;
- Do not make LLM Wiki a core dependency;
- Implement the interfaces needed for future integration now.

Code Intelligence must already implement:

```text
WikiBinding
WikiConnector interface
wiki.mode none/local/remote
Hub Wiki Binding UI
pending Wiki update model
agent-context representation
Knowledge Hub capability payload
```

Adaptations **inside the LLM Wiki system itself** belong to a separate integration phase after Code Intelligence has been verified.

## 2.3 Connector contract

Define an implementation-independent interface:

```text
WikiConnector
- detect_capabilities()
- get_context(project_binding)
- get_update_instructions()
- submit_durable_update(payload)
- get_update_status(id)
- health()
```

The core does not know how LLM Wiki stores Markdown, graphs or reviews.

Only the adapter knows.

## 2.4 Local Wiki

`wiki.mode=local` does not require a central Wiki Hub.

The Worker may read only knowledge files explicitly allowed as Wiki context by policy.

Do not create a code graph inside the Wiki.

## 2.5 Remote Wiki

Remote mode is intended for future API/MCP integration with an existing knowledge hub.

When the remote Wiki is unavailable:

```text
code quality pipeline = AVAILABLE
wiki_context = UNAVAILABLE/PARTIAL
wiki_update = PENDING/NOT_ATTEMPTED
```

Do not turn `quality_gate` into FAIL solely because of the Wiki.

---

# 3. Required V1 technology stack

The goals are low RAM usage, portability, straightforward maintenance and a small failure surface.

## 3.1 Worker / Core

**Language:** Stable Rust.

Build the Worker, CLI, local protocol client and MCP adapter from the same Rust workspace.

Recommended structure:

```text
crates/
├── codeintel-core
├── codeintel-graph
├── codeintel-indexer
├── codeintel-semantic
├── codeintel-frameworks
├── codeintel-review
├── codeintel-runtime
├── codeintel-quality
├── codeintel-worker
├── codeintel-mcp
└── codeintel-cli
```

Requirements:

- Use async only where it provides meaningful value;
- Never keep a full graph copy in memory;
- No persistent Python daemon as part of the Worker;
- External plugins may run as bounded subprocesses;
- A panic within one analysis must not bring down the Worker without recovery;
- Every heavy task has a timeout and resource observation.

## 3.2 Worker Local Persistence

Core V1 uses **SQLite only**.

Settings:

```text
journal_mode = WAL
foreign_keys = ON
synchronous = NORMAL
busy_timeout = 5000ms
```

Critical migration/publication operations may temporarily use `FULL` when necessary.

V1 has no mandatory dependency on:

- Memgraph;
- Neo4j;
- Qdrant;
- Redis;
- Elasticsearch;
- Ollama;
- Remote LLM API.

## 3.3 Hub Backend

**Python 3.12+ / FastAPI**

Layers:

```text
hub/
├── api/
├── auth/
├── services/
├── repositories/
├── models/
├── migrations/
├── jobs/
├── mcp/
├── docs/
└── ui/
```

DB access:

- SQLAlchemy 2 style;
- Alembic migrations;
- typed Pydantic request/response models;
- Business logic does not live inside route handlers.

## 3.4 Hub Database

V1:

**SQLite WAL**.

PostgreSQL is not required.

The repository layer must avoid unnecessary SQLite-specific SQL so that a future PostgreSQL transition is a migration rather than a rewrite.

V1 does not use Redis.

The Hub job queue is database-backed.

## 3.5 Hub UI

**Server-rendered UI: Jinja2 + HTMX + minimal vanilla JavaScript.**

V1 has no separate React/Vue/Next frontend.

Implementation rationale:

- Lower RAM usage;
- A smaller build pipeline;
- Fewer dependencies;
- Simpler authentication and sessions;
- The UI is an administrative/self-service portal rather than a large public SPA.

CSS:

- Plain CSS or a small local utility bundle;
- No required Node runtime in the production Hub.

## 3.6 Production Reverse Proxy

The Hub application is not exposed directly to the internet.

Bundled deployment reference:

Use **Nginx** as the reverse proxy.

If a host already has a suitable reverse proxy, the Hub may use it instead of another Nginx instance, provided all TLS, header and timeout requirements are preserved.

This is a deployment adaptation, not a product stack change.

## 3.7 Hub Packaging

Bundled deployment:

```text
Docker Compose
├── hub-api
├── reverse-proxy (when not using existing host proxy)
└── persistent volumes
```

The SQLite database, uploads/release artifacts and master-key mount must persist.

---

# 4. Worker ↔ Hub transport — final V1 decision

V1 uses **outbound HTTPS long polling**.

WebSocket is not required for V1.

## 4.1 Rationale

The choice provides:

- firewall/NAT friendliness;
- no inbound Worker port;
- Straightforward recovery;
- Less complex connection state;
- Easier debugging;
- Support for small to medium Worker fleets.

## 4.2 Flow

```text
Worker
  ↓ HTTPS
heartbeat/report sync
  ↓
long-poll for jobs
  ↓
Hub
```

Worker:

1. Send a heartbeat.
2. Synchronize the spool.
3. Open a long poll for a job.
4. If there is no job, the Hub returns no-content/an empty result after the timeout.
5. Repeat.

## 4.3 Timeouts

V1 defaults:

```text
TCP/connect timeout:        5s
normal API request:        30s
long-poll server hold:     25s
long-poll client timeout:  35s
remote analysis job:       async, no request kept open for job duration
```

## 4.4 Retry

Retry only idempotent/replay-safe operations.

Backoff:

```text
1s → 2s → 4s → 8s → 15s → 30s
+ jitter
```

Never use a tight retry loop.

## 4.5 Future

V2 may add WebSocket through a transport adapter without changing the Worker core or Hub domain model.

---

# 5. Local Worker IPC / MCP

## 5.1 Worker Daemon

`codeintel worker` runs as a systemd service.

It owns:

- project registry local;
- index jobs;
- graph DB;
- spool;
- runtime state.

## 5.2 Unix Socket

Privileged/local control communication defaults to a Unix domain socket:

```text
/run/code-intelligence/worker.sock
```

Permissions are restricted to the service user and configured local users.

The local Worker must not bind a public HTTP port.

## 5.3 Local MCP

Local MCP for agents uses **stdio**.

The same binary:

```bash
codeintel mcp serve --stdio
```

The MCP process is a lightweight adapter communicating with the Worker daemon over the Unix socket.

This provides:

```text
Codex/Hermes/Claude
→ MCP stdio
→ local Unix socket
→ Worker
→ current source + graph
```

The MCP process does not build another graph.

---

# 6. Remote Hub MCP

Remote Hub MCP is a read/control interface separate from local MCP.

Core contracts do not depend on the MCP transport version.

Requirements:

- Project-scoped authentication;
- bounded responses;
- no Full Source by default;
- async live-analysis request IDs;
- Context Pack;
- current/stale markers.

Isolate remote MCP transport inside an adapter so future protocol changes do not require core/database/Worker changes.

---

# 7. API conventions — mandatory

Base:

```text
/api/v1/
```

## 7.1 Encoding

- JSON UTF-8.
- field names: `snake_case`.
- timestamps: UTC RFC3339.
- IDs: UUIDv4.
- Durations: integer milliseconds or explicit `_sec` / `_ms` fields.
- Sizes: bytes or fields with a clear unit suffix.

## 7.2 Request ID

Every request receives:

```http
X-Request-ID
```

If the client does not supply one, the Hub creates it.

Every log, audit entry and error includes `request_id`.

## 7.3 Error Envelope

All API errors are structured:

```json
{
  "error": {
    "code": "SOURCE_REVISION_STALE",
    "message": "The evidence does not match the current source revision.",
    "request_id": "uuid",
    "details": {
      "expected_revision": "...",
      "actual_revision": "..."
    },
    "retryable": false
  }
}
```

`code` is machine-readable and stable within a MAJOR version.

Never return HTML errors to API clients.

## 7.4 HTTP Semantics

```text
200 OK             read/update success
201 Created        new durable object
202 Accepted       asynchronous job accepted
204 No Content     successful operation with no body / empty long-poll where appropriate
400 Bad Request    malformed request
401 Unauthorized   missing/invalid authentication
403 Forbidden      authenticated but scope denied
404 Not Found      object not visible/found
409 Conflict       revision/lock/idempotency conflict
412 Precondition   stale precondition/version mismatch where applicable
422 Validation     valid JSON but invalid domain input
429 Too Many       rate limit
500 Internal       unexpected server error
503 Unavailable    dependency/maintenance temporary unavailable
```

## 7.5 Pagination

Cursor-based.

```text
default limit: 50
max limit:     200
```

Response:

```json
{
  "items": [],
  "next_cursor": null
}
```

Do not use offset pagination for large history/event tables.

## 7.6 Idempotency

Worker POST requests for:

- reports;
- evidence;
- heartbeats where relevant;
- spool replay;
- remote analysis result;

must be replay-safe.

The client generates a stable object UUID.

Critical create operations support `Idempotency-Key`.

Duplicate delivery must not create duplicate records.

## 7.7 Optimistic Concurrency

Updates to mutable administrative resources use a version/ETag.

Example:

```http
If-Match: "<version>"
```

Conflicts return `409` or `412`.

## 7.8 Bounded Payloads

Every endpoint defines:

- max request body;
- max result items;
- max log/stdout bytes;
- max context output.

No endpoint may return an unbounded raw graph.

---

# 8. Database Rules — Hub

## 8.1 Migrations

Alembic migration IDs sequential/reviewable.

Production must not use `create_all()` as a substitute for migrations.

Each release:

1. backup DB;
2. validate migration path;
3. Migrates transactionally where possible;
4. health check;
5. Documents its rollback procedure.

## 8.2 Foreign Keys

`PRAGMA foreign_keys=ON`.

Do not intentionally create orphan rows.

Renaming must not automatically delete historical objects.

By default, projects/Workers are retired through `retired_at`/status rather than hard-deleted.

## 8.3 Immutability

Evidence for one revision must not be updated to represent evidence for another revision.

Quality/analysis runs are historical records.

Metadata may be corrected, but original provenance is retained.

## 8.4 Required Index Families

Create database indexes at least on:

```text
project_instances(project_id, status)
source_revisions(project_id, instance_id, created_at)
analysis_runs(project_id, instance_id, source_revision_id, completed_at)
quality_runs(project_id, instance_id, source_revision_id, completed_at)
findings(project_id, status, severity, updated_at)
evidence(project_id, instance_id, source_revision_id, origin)
remote_analysis_requests(project_id, status, created_at)
audit_events(created_at)
agent_credentials(scope_type, scope_id, status)
```

Developers may add indexes based on measurements.

## 8.5 SQLite Maintenance

Hub and Worker:

- periodic WAL checkpoint;
- Run integrity checks on schedule and during upgrades;
- Run `VACUUM` only during controlled maintenance, never during an active task;
- Restrict database file permissions.

---

# 9. Worker Graph DB Rules

Graph publication must be atomic.

A partially built graph must never receive `CURRENT` status.

Index lifecycle:

```text
BUILDING
→ VALIDATING
→ CURRENT
```

or:

```text
BUILDING
→ INVALID
```

Keep the previous valid snapshot queryable until the new one is published whenever possible.

Corruption:

```text
detect
→ mark INVALID
→ preserve diagnostic metadata
→ rebuild derived DB
```

Rebuilding must not change source.

---

# 10. Authentication — Owner UI

## 10.1 Password

Password hashing:

**Argon2id**.

Requirements:

- unique salt;
- library-maintained encoded hash format;
- Parameters can be upgraded;
- Successful login may transparently rehash a password when policy changes.

Do not use:

- plaintext password;
- SHA-256 password hashing;
- reversible password encryption.

## 10.2 Session

Browser authentication uses server-side sessions.

Cookie:

```text
Secure
HttpOnly
SameSite=Lax
Path=/
```

Session ID:

- At least 256 random bits;
- Never store the raw value as plaintext in the database;
- Regenerate the session after login;
- Logout invalidates the server-side session.

Timeouts:

```text
idle timeout: 12 hours
absolute max: 7 days
```

These settings may be hardened later.

## 10.3 CSRF

Every browser state-changing request (`POST/PUT/PATCH/DELETE`) requires CSRF protection.

Bearer-token API calls do not use the cookie-based CSRF mechanism.

## 10.4 Login Rate Limit

Default:

```text
5 failed attempts / 15 minutes / account + source IP
```

Then apply exponential delay or a temporary lock.

Never apply a permanent lockout requiring database editing.

Record the security event in the audit log.

## 10.5 Optional TOTP

TOTP remains optional under the parent specification.

When enabled:

- Single-use recovery codes;
- Display codes once only;
- stored hashed;
- disable requires owner re-authentication.

---

# 11. Tokens, secrets and encryption

The main specification requires reveal/copy/rotate/revoke.

Therefore, token values cannot all be stored solely as one-way hashes.

## 11.1 Token Generation

Tokens:

- cryptographically secure random;
- At least 256 bits of entropy;
- A prefix identifying type/version without disclosing the secret.

Conceptual example:

```text
ci_proj_v1_<secret>
```

## 11.2 Storage

The Hub stores:

```text
token_id
prefix
type
scope
created_at
expires_at
last_used_at
status
validation_hash
encrypted_secret
encryption_key_id
```

Encrypt `encrypted_secret` using AES-256-GCM.

Store a separate keyed validation hash/HMAC for efficient lookup and validation.

## 11.3 Master Encryption Key

The master key must not reside in the Hub database.

Default deployment:

```text
/etc/code-intelligence/hub-master.key
```

- Readable only by root;
- Mounted read-only into the Hub container;
- permissions `0600`;
- Never committed to Git;
- Never printed in logs.

## 11.4 Key Backup

Back up the master key **separately from the database**.

A lost database can be restored from its database backup.

Losing the encryption key can make existing encrypted secrets unrecoverable.

The production handoff must therefore include:

```text
BACKUP DATABASE
+
BACKUP MASTER KEY
```

in separate locations.

## 11.5 Key Rotation

Vault records include `encryption_key_id`.

Rotation:

1. Create a new key.
2. Keep old and new keys available temporarily.
3. Use the new key for new writes.
4. batch re-encrypt.
5. verify.
6. Retain the old key for a recovery window.
7. Retire it after backup and verification.

Encryption-key rotation does not change token values unless the user also chooses token rotation.

## 11.6 Reveal

Reveal requires:

- owner authenticated;
- Recent reauthentication when the session is old;
- audit event;
- No secret in URLs/query strings;
- No secret in server logs.

The UI must not leave tokens visible after navigation/reload without another Reveal action.

---

# 12. Security Headers — Hub

The reverse proxy/Hub must configure at least:

```text
Strict-Transport-Security
X-Content-Type-Options: nosniff
Referrer-Policy: no-referrer
Content-Security-Policy
frame-ancestors 'none'
```

CSP allows only resources actually required by the Hub.

CDN-hosted JavaScript is not required.

Administrative pages must not be embeddable in iframes.

---

# 13. TLS

Production Hub:

- HTTPS only;
- HTTP → HTTPS redirect;
- invalid/expired certificate = deployment health failure;
- The Worker never disables TLS verification;
- No `curl -k` in installer or Worker production paths.

---

# 14. API / Agent Credential Rules

Scopes are explicit.

Examples:

```text
hub:read
project:<id>:read
project:<id>:live_analysis
worker:<id>:sync
admin
wiki:<binding>:connect
```

`PROJECT_AGENT` does not receive administrative access.

A Worker credential is not an Owner UI token.

Token revocation takes effect without a restart.

---

# 15. Backup Policy — Hub

The Hub contains non-derived information, so backups are mandatory.

## 15.1 Backup scope

Required:

- Hub SQLite DB;
- Hub master encryption key, backed up separately;
- Hub configuration without embedded secrets;
- Any release/documentation artifacts that cannot be regenerated from the source repository.

Worker graphs do not require backup.

## 15.2 Schedule

Default V1:

```text
daily DB backup
+
backup immediately before every DB migration/upgrade
```

## 15.3 Retention

Default:

```text
14 daily
8 weekly
6 monthly
```

Configurable through settings.

## 15.4 Backup Consistency

Back up SQLite through its online backup API or another consistent snapshot method.

Never copy a live database in a way that could produce an inconsistent backup.

## 15.5 Encryption

Encrypt backups containing sensitive database data at rest.

Store key backups separately from database backups.

## 15.6 Restore Test

Required:

- A restore test before declaring production readiness;
- A restore test before major releases involving significant migrations;
- Periodic restore verification, recommended at least monthly.

Restore tests verify:

```text
DB opens
migrations valid
owner login works
token vault decrypts
projects/history visible
workers can re-enroll/reconnect
```

---

# 16. Worker Local Data Retention

Worker storage is mostly derived and rebuildable.

Cleanup order remains as defined in the parent specification.

Additionally:

- raw runtime traces: short retention, default parent policy;
- content cache: bounded LRU;
- old graph snapshots: keep only as needed for safe publication/debugging;
- Never delete spool entries before Hub acknowledgment;
- Report summaries may remain local until synchronized;
- Low disk space never causes deletion of source or `.code-intel/`.

---

# 17. Release Format

## 17.1 Versioning

The product uses Semantic Versioning:

```text
MAJOR.MINOR.PATCH
```

Tag:

```text
v1.2.3
```

## 17.2 Artifacts

Naming:

```text
codeintel-worker-v1.2.3-linux-amd64.tar.gz
codeintel-worker-v1.2.3-linux-arm64.tar.gz
codeintel-hub-v1.2.3.tar.gz
codeintel-agent-adapters-v1.2.3.zip
codeintel-docs-v1.2.3.zip
install.sh
SHA256SUMS
release-manifest.json
release-manifest.sig
```

## 17.3 Release Manifest

Including:

```json
{
  "product_version": "1.2.3",
  "agent_policy_version": 4,
  "worker_mcp_schema_version": 3,
  "hub_mcp_schema_version": 2,
  "project_manifest_schema_version": 1,
  "artifacts": []
}
```

## 17.4 Integrity

Every artifact receives a SHA-256 checksum.

The installer verifies checksums before installation.

## 17.5 Signing

Sign release manifests with Ed25519.

- Never store the private signing key in the Hub database;
- Embed/package the public verification key in the installer;
- An invalid signature blocks automatic installation/updates.

Checksums alone do not replace signatures when release signing is enabled in production.

---

# 18. Compatibility Policy

## 18.1 Product Major

Hub and Worker with different MAJOR versions:

```text
INCOMPATIBLE
```

Local analysis may continue when the Worker's local database/schema is valid, but Hub synchronization/live relay is explicitly blocked or limited.

No silent best-effort fallback.

## 18.2 Minor Compatibility

V1 policy:

- The Hub supports the current and previous minor versions within the same major when approved by the compatibility manifest.
- Display version mismatches in the UI.
- Mark outdated Workers `UPGRADE_REQUIRED`.
- Never hide schema mismatches.

## 18.3 Adapter Compatibility

Codex/Claude/Hermes adapters are versioned separately in the release manifest.

`codeintel doctor` checks:

- client detected;
- config path;
- MCP launch;
- policy version;
- adapter version.

---

# 19. Upgrade Procedure

Worker:

```text
download
→ verify signature/checksum
→ stop worker
→ backup local DB metadata if migration is destructive
→ atomic binary replace
→ schema migration
→ start
→ doctor
→ mark incompatible indexes REBUILD_REQUIRED
→ lazy/local rebuild
```

Hub:

```text
maintenance marker
→ DB backup
→ verify master-key availability
→ deploy compatible image
→ run migration
→ health checks
→ smoke admin/API
→ clear maintenance marker
```

If health checks or migrations fail:

```text
rollback application
+
restore compatible DB backup when schema rollback is unsafe
```

Do not continue with a partially migrated database.

---

# 20. Logging

Structured logs.

Required fields:

```text
timestamp
level
service
request_id
project_id (when applicable)
instance_id (when applicable)
change_run_id (when applicable)
event_code
message
```

Do not include:

- passwords;
- token values;
- encryption keys;
- session IDs;
- unbounded source/code dumps;
- full environment with secrets.

Default local retention:

- log rotation;
- bounded size;
- configurable days.

---

# 21. Observability

Hub System Health displays at least:

```text
Hub API
DB
migration version
release version
job queue
worker online/offline
last worker heartbeat
sync backlog
failed jobs
disk
backup freshness
last successful backup
last restore-test result
```

Worker:

```text
version
idle RSS
peak recent RSS
active project
index state
graph DB size
spool size
heavy job status
last Hub sync
local doctor result
```

---

# 22. Health Endpoints

Hub:

```text
GET /health/live
GET /health/ready
```

Worker local:

```bash
codeintel doctor
codeintel status
```

If a local HTTP health endpoint exists, bind it only to localhost/a Unix socket.

`ready` must not be healthy when a migration is required or the database is unavailable.

---

# 23. Job Queue Semantics

Hub DB-backed queue.

Job states:

```text
PENDING
CLAIMED
RUNNING
SUCCEEDED
FAILED
EXPIRED
CANCELLED
```

Claims include a lease timeout.

Worker crash:

```text
lease expires
→ job requeue if replay-safe
```

Do not execute destructive remote jobs through the V1 live relay.

Remote analysis is read-only analysis.

---

# 24. Audit

Audit the following:

- login success/failure summary;
- token create;
- token reveal;
- token rotate;
- token revoke;
- Worker enrollment/revoke;
- remote analysis request;
- project recover/rebind;
- owner security setting changes;
- release/upgrade action;
- backup/restore administrative action.

Audit events contain no secrets.

---

# 25. Repository / Monorepo Structure

Recommended pins:

```text
code-intelligence/
├── worker/
│   ├── crates/
│   ├── migrations/
│   └── tests/
├── hub/
│   ├── app/
│   ├── migrations/
│   ├── templates/
│   ├── static/
│   └── tests/
├── protocol/
│   ├── schemas/
│   └── compatibility/
├── adapters/
│   ├── codex/
│   ├── claude/
│   ├── hermes/
│   └── generic-mcp/
├── docs/
├── installers/
├── deploy/
│   ├── docker-compose/
│   ├── nginx/
│   └── systemd/
├── fixtures/
├── benchmarks/
├── scripts/
├── ADR/
├── IMPLEMENTATION_STATUS.md
└── CHANGELOG.md
```

---

# 26. Protocol Schemas

Hub–Worker payloads must be schema-versioned.

Never use Python/Rust internal object serialization directly as a network protocol.

V1 uses JSON contracts.

Every primary message includes:

```text
schema_version
message_id
sent_at
```

For project-related messages:

```text
project_id
instance_id
source_revision_id
```

where applicable.

Tolerate unknown future fields when safe.

An unknown enum affecting semantics produces explicit incompatibility, never silent fallback.

---

# 27. Source revision safety — unchanged

These implementation decisions do not alter the parent invariants:

```text
actual current source = truth
graph = derived
tests = validation
runtime = observed evidence
wiki = durable knowledge
```

All quality evidence remains pinned to `source_revision_id`.

Never automatically remap a PASS to a new revision.

---

# 28. Graph Distrust — Implementation Tests

In addition to the main acceptance tests, unit/integration tests must verify:

```text
empty graph query
→ absence_proven=false
```

```text
heuristic edge
→ never exact
```

```text
source changed
→ previous evidence stale
```

```text
runtime not observed
→ NOT_OBSERVED
```

```text
unsupported semantic area
→ PARTIAL/UNKNOWN
```

No prompt can override core semantics.

---

# 29. Security boundary — what remains out of scope

The hardening in this document does not make Code Intelligence a sandbox.

The following remain out of scope under the main specification:

- Restricting Codex root privileges;
- Comprehensive VPS OS hardening;
- arbitrary source-code prompt injection defense;
- A full supply-chain scanner for users' projects;
- deployment authorization;
- snapshot provider integration.

However, **security of the Hub itself is mandatory**.

---

# 30. UI Security UX

The Hub serves a single owner, so the UX should be simple without compromising security.

Required:

- logout;
- Clear session expiration;
- Explicit clicks to reveal tokens;
- destructive/revoke confirmation;
- never put token in HTML page source before reveal;
- Disable browser autocomplete on secret-reveal fields where possible;
- security/audit page;
- current app version;
- last backup status;
- Worker compatibility warnings.

---

# 31. Documentation Single Source

Maintain the canonical agent policy once in the repository.

Derive the following from it:

```text
Hub rendered Markdown
Raw Markdown
Skill
MCP initialize instructions
Project bootstrap
downloadable docs bundle
```

Do not maintain copied policy forks across multiple templates.

Builds/tests must detect generated documentation that is out of sync.

---

# 32. Agent Documentation

The Hub displays at least:

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

Also add:

```text
SECURITY_AND_SESSIONS.md
BACKUP_AND_RESTORE.md
VERSION_COMPATIBILITY.md
IMPLEMENTATION_ARCHITECTURE.md
```

Every document:

- rendered;
- raw Markdown;
- versioned;
- downloadable.

---

# 33. Project Initialization UX

`codeintel init`:

1. Detects languages/frameworks.
2. Suggests source roots.
3. Suggests ignore rules.
4. Detects existing checks.
5. Detects possible smoke endpoints without inventing assertions.
6. Asks for/configures Wiki mode.
7. Defaults the Wiki to none.
8. Creates `.code-intel/`.
9. Builds the initial index.
10. Registers an instance in the Hub when available.
11. Runs `doctor`.
12. Displays a final setup summary.

An agent can complete the entire wizard independently.

---

# 34. `.code-intel/` Portability

Store only portable inputs inside the project:

```text
.code-intel/
├── project.yaml
├── policy.yaml
└── AGENT_POLICY.md
```

By default, do not store:

- graph DB;
- runtime traces;
- LSP cache;
- secrets;
- Worker credential;
- Hub owner token.

This keeps VPS migration as:

```text
move source + .code-intel
→ Worker detects project_id
→ rebuild derived state
→ register new ProjectInstance
```

---

# 35. Resource Decisions

With limited RAM and 20–30 GB of available storage:

## 35.1 RAM

- Worker idle usage remains low under the parent targets;
- heavy jobs default concurrency = 1;
- one LSP process at a time;
- runtime collector on-demand;
- Keep the default Hub process count small;
- Never preload graphs for every project into memory.

## 35.2 Disk

Derived data may be deleted/rebuilt.

Do not treat 20–30 GB as a target consumption level.

Preserve the minimum free-disk reserve defined by parent policy.

## 35.3 Hub

Hub metadata/history should remain relatively small, but retention rules still apply.

---

# 36. Quality Gate Independence from Wiki

`quality_gate` does not require “Wiki updated successfully” to return PASS.

Example:

```text
Code checks PASS
Wiki remote unavailable
```

Result:

```text
Quality Gate: PASS
Wiki Sync: PENDING/UNAVAILABLE
```

Not:

```text
Quality Gate: FAIL
```

unless a future project policy explicitly defines a special business requirement beyond the default.

---

# 37. Project context pack without a Wiki

Without a Wiki, the Hub still creates a technical context pack:

- A brief purpose, if provided;
- languages/frameworks;
- architecture summary derived;
- entry points;
- current health;
- recent validated changes;
- known findings;
- checks;
- runtime coverage;
- last source revision;
- Agent instructions.

A context pack does not become another Wiki.

It is an operational derived summary.

---

# 38. Preparing for future LLM Wiki integration

When adapting LLM Wiki in the future:

The following must not require changes:

- graph schema;
- review engine;
- risk engine;
- runtime;
- Quality Gate;
- Worker source indexing;
- Project identity;
- ChangeRun.

Integration should consist primarily of:

```text
WikiConnector adapter
+
Knowledge Hub capability
+
conditional Wiki prompt
+
authentication/binding
+
durable update mapping
+
UI link/summary
```

If connecting a Wiki requires a core rewrite, the architecture fails this contract.

---

# 39. Additional acceptance tests for this companion document

In addition to every acceptance test in the main specification:

## Stack / Deployment

1. The Worker release is a Rust binary and core features require no Python runtime.
2. The Hub starts from Docker Compose with persistent SQLite storage.
3. The Hub works without Redis.
4. Local Worker analysis works while the Hub is down.
5. Memgraph/Qdrant/Ollama are not required services.

## Transport

6. The Worker connects outbound only.
7. Long polling reconnects after a normal timeout.
8. Hub restart requires no manual Worker restart.
9. Duplicate reports after reconnect create no duplicate history.
10. The spool is resent idempotently.

## Auth

11. A password database leak does not reveal plaintext passwords.
12. Session cookies use Secure, HttpOnly and SameSite.
13. State-changing browser requests without CSRF tokens are rejected.
14. Login rate limiting is enforced.
15. Revoked API tokens stop working.
16. Token Reveal is audited.
17. Token values never appear in logs.
18. The master encryption key is not stored in the database.

## Backup

19. The daily backup job can run.
20. A pre-migration backup is created.
21. Restoring the database with the correct master key successfully decrypts tokens.
22. Restoring without the key reports a clear failure without corrupting the vault.
23. Restore testing is documented before production readiness.

## Release

24. Checksum mismatches block installation/updates.
25. Invalid release signatures block automatic installation/updates.
26. A Hub/Worker major mismatch is displayed as INCOMPATIBLE.
27. Supported minor mismatches are displayed but work according to the compatibility matrix.
28. Failed Worker upgrades can return to the previous binary.
29. Failed Hub migrations activate the rollback/restore path.

## Wiki independence

30. A project with `wiki.mode=none` completes the entire code workflow.
31. A disabled Wiki connector does not affect quality gates.
32. An unavailable remote Wiki does not block local code work.
33. Project context packs exist without a Wiki.
34. Enabling future Wiki bindings does not change `project_id`.
35. Enabling a Wiki binding does not require a graph rebuild.

---

# 40. Additional production-readiness checklist

Before handoff:

- [ ] The parent specification is fully satisfied.
- [ ] All stack decisions in this document are implemented.
- [ ] Hub security controls pass tests.
- [ ] Database and key backups are documented and tested.
- [ ] A restore test has been completed.
- [ ] Release checksum/signature verification works.
- [ ] Worker outbound long polling is stable.
- [ ] Worker-offline and Hub-offline behavior is tested.
- [ ] `wiki.mode=none` passes end to end.
- [ ] WikiConnector interfaces exist even if external adaptation follows in a later phase.
- [ ] There is no hidden LLM Wiki dependency.
- [ ] There is no hidden GitHub dependency.
- [ ] No heavy daemon is mandatory.
- [ ] No-false-PASS invariants pass.
- [ ] `codeintel doctor` passes after a fresh installation.
- [ ] install/uninstall/update idempotent.
- [ ] Backup freshness is displayed in the Hub.
- [ ] Version/compatibility are displayed in the Hub.
- [ ] Developer handoff includes key/backup/recovery instructions without revealing secrets.

---

# 41. Controlled Exceptions

Exceptions to this document are permitted only when:

1. A production environment limitation is proven;
2. Every requirement of the main specification is preserved;
3. An ADR is written;
4. The following are documented:
   - problem;
   - alternatives;
   - RAM/disk/token/security impact;
   - migration impact;
   - rollback;
5. The exception is approved before merging a core architecture change.

“I know this library better” is not sufficient justification by itself.

---

# 42. Definition of done for this document

Implementation decisions are complete only when:

1. Two independent developers/agents reading both documents would build the same macro-stack.
2. V1 leaves no open choice between Redis/no Redis, heavy graph databases/SQLite or WebSocket/long polling.
3. Hub security is not left to discretion.
4. Backup/restore is implemented rather than merely requiring a future guide.
5. Release/upgrade/compatibility are machine-readable.
6. The Worker remains portable and local to source.
7. The Wiki is an optional integration, not a core dependency.
8. The system works for code-only projects without an LLM Wiki.
9. The architecture already supports future Wiki integration without a core rewrite.
10. Every decision remains subject to the main specification's source truth, graph distrust and quality gates.

---

# 43. Final developer instruction

Read this document **together with**:

`CODE_INTELLIGENCE_FINAL_PRODUCTION_SPEC_V1.md`

Do not treat this document as a list of suggestions.

These are binding V1 implementation decisions unless the main specification requires otherwise.

Build the system so users can select, per project:

```text
Code Intelligence only
```

or:

```text
Code Intelligence + local LLM-Wiki
```

or:

```text
Code Intelligence + remote LLM-Wiki / Knowledge Hub
```

without replacing Code Intelligence core or activating irrelevant workflows.

The goal is one stable, RAM-efficient production product, as deterministic as practical, that improves agent code work both without a Wiki and with future Wiki integration for durable project memory and knowledge.


# Changelog

## 1.3.0 — authorized security scenarios and controlled environment drift

- Added a local-only, revision-bound endpoint security matrix for authorization, BOLA/IDOR, input,
  replay, bounded rate-limit and paid-API budget assertions.
- Added an opt-in parallel atomicity probe limited to `rapid_repeat`, four workers and five requests;
  this can expose quota/provider-budget check-then-act races without enabling load testing.
- Added exact route-to-runtime security correlation with bounded source-free downstream code paths;
  missing or ambiguous graph matches remain UNKNOWN.
- Added security finding recall/precision to the paired A/B benchmark and a non-evidentiary abuse
  review template, so platform expansion depends on measured value rather than tool usage.
- Added embedded Rust indexing and changed-scope semantic gating; unrelated global analyzer gaps warn
  without deadlocking a valid current-revision change.
- Made the explicit `codeintel index` command rebuild a staged graph even when source bytes are
  unchanged, so a reviewed analyzer upgrade cannot silently reuse old derived evidence.
- Bounded review output to 12 excerpts of 800 characters and compact de-duplicated graph metadata;
  large one-line assets no longer consume the agent context as accidental source-body dumps.
- Added per-item caps to coverage/dynamic-boundary samples, preventing a single minified source line
  from defeating the MCP response budget.
- Added hard authorization, environment, allowlist, expiry, request-count, concurrency, redirect,
  response-retention and double state-change gates. Tool calls cannot inject targets or credentials.
- Added an offline environment baseline/audit and local Rust toolchain discovery with no downloads or
  automatic updates.
- Added recoverable ChangeRun abandonment so a deliberately stopped or permanently UNKNOWN run cannot
  deadlock later project work; prior reports are preserved with a reason.
- Reviewed reverse-skill at a pinned commit and retained only defensive process concepts; no upstream
  executable, payload, self-modifying or dynamic-download material is bundled.

## 1.2.1 — deterministic graph-node identity collision fix

- Replaced truncated name-based node identities with versioned, source-derived canonical node
  identities that include logical root, relative path, language, kind, qualified container and
  source range.
- Added explicit provenance merging for repeated discovery of the same canonical entity and
  collision diagnostics for genuinely different candidates. Failed staged graphs are never
  published as CURRENT.

## 1.2.0 — local-first access simplification and portable project intelligence

- Replaced the default token-per-role workflow with two owner-facing concepts: Host Access for
  provisioning and one Project Access shared by authorized Codex, Claude, Hermes and MCP clients.
- Separated Host provisioning from the internal Worker sync credential, so disabling new Project
  creation does not interrupt existing Project reports, heartbeat or local analysis.
- Made Project Access available directly on every Project page with Reveal/Copy/Rotate/Revoke/Test,
  full Agent setup, project endpoints, Agent Kit and client/time activity metadata.
- Added secure local Project binding under `.code-intel/local/`, explicit same-project bind versus
  fork behavior, and portable Skill/context/graph-rebuild metadata that travels with Git without
  secrets or derived graph databases.
- Added first-class local-only installation and clarified that Graph, Review, Runtime, Data-flow and
  exact-revision Quality Gate never depend on Hub availability or credentials.
- Consolidated installation and operational instructions around one conditional universal Agent
  prompt while retaining scenario guides as concise entry points.

This release does not claim Product Production Stable until the complete acceptance matrix, physical
multi-host pilots and required Codex A/B benchmark are recorded.

## 1.1.1 — remote reader UX, scalable lists and live graph repair

- Added a per-project Remote Reader Kit with separate read-only and live-analysis credentials,
  explicit REST/MCP endpoints, a browser-built plaintext prompt and in-project Reveal/Rotate/Revoke.
- Kept remote research/presentation mode explicitly separate from the source-local coding workflow,
  preventing central context from being represented as current line-level code verification.
- Added six Hebrew nontechnical guides with visual flows and full copyable Agent prompts for Host
  install, onboarding, normal code work, migration, Wiki binding and remote presentation work.
- Added server-side 25-row pagination to growing Hub tables and name/slug search for hundreds of
  projects.
- Fixed the on-demand 3D graph relay payload mismatch, lease recovery and browser polling window.
- Fixed relative CLI paths so `codeintel open .` is resolved by the caller before reaching the
  systemd daemon.

The operational deployment is hardened, but the product-level Production Stable declaration remains
withheld until the complete acceptance matrix, real multi-host pilots and required Codex benchmark pass.

## 1.1.0 — onboarding and operational hardening

- Locked Project activation to explicit opt-in; the global Skill and MCP initialize policy remain
  non-invasive for unmarked and Wiki-only projects.
- Made `codeintel init` concurrency-safe and idempotent, with automatic Project/Instance registration,
  explicit Local/Hub/Wiki state, durable offline pending state and automatic post-restart retry.
- Added first-class registration/sync fields to Hub models, API, dashboard and Project views.
- Added a universal Hebrew Agent bootstrap Prompt, plaintext-on-demand token guides, direct per-project
  token creation, favicon and nontechnical installation/onboarding guidance.
- Added a signed Complete Agent Kit ZIP built from the same release as the curl installer, with Linux
  amd64/arm64, offline install, Windows/WSL2 wrapper, adapters, Skill, docs and checksums.
- Added configured multi-user Agent installation, preserving existing client config and local MCP access.
- Added bounded on-demand 3D graph visualization: no source bodies, Worker-online only, short expiry,
  no-cache and consume-once Hub payload semantics.
- Fixed signed `SHA256SUMS` extraction in both installer and updater and added automated Online/Offline
  onboarding, concurrent init, duplicate registration and ephemeral graph acceptance coverage.

This release is deployable as an operational hardening build. The product-level Production Stable
declaration remains withheld until the complete 82+35 acceptance matrix, cross-host pilots and the
required 20–30 task Codex A/B benchmark are recorded.

## 1.0.1 — validation build

- Dedicated semantic and framework crates with scoped unknown handling.
- On-demand runtime adapter contract with OTLP/HTTP normalization, graph persistence and timeout cleanup.
- Advanced bounded graph evidence, acceptance fixtures and strict benchmark harness.
- Coverage responses now use bounded samples and explicit `items_remaining` instead of losing the full response on truncation.
- Installer reruns now preserve Worker identity and credentials; Worker updates use a pinned release
  key, atomic binary replacement, bounded socket readiness and automatic rollback.
- Data-flow reachability now excludes structural-only graph edges, accepts current-revision runtime
  observations, and permits `NO_FLOW` only for a declared and analyzed complete scope.
- Added failure-injection coverage for failed atomic graph publication, corrupt derived SQLite
  replacement, runtime collector timeout cleanup and historical spool replay.
- Full Rust Clippy cleanup and expanded self-verification policy.

This build remains a validation build until the complete acceptance, failure-injection, pilot and
20–30 task Codex benchmark programs pass.

## 1.0.0 — validation build

- Unified Hub, Worker, local/remote MCP, source revision, graph, risk, review, verification, and gate.
- Hebrew owner dashboard, Agent Center, downloads, documentation, and encrypted token management.
- Git-optional project portability and Wiki-independent operation.
- Added a dedicated hybrid semantic resolver with import/type evidence, preserved ambiguity,
  dynamic-boundary reporting and changed-scope verification.
- Added a framework registry for Laravel, WordPress, Express, Next.js, FastAPI, Django and Docker Compose.
- Added bounded advanced graph/path/edge-evidence queries, no-Git/no-Wiki fixtures, and a strict
  Codex A/B benchmark harness that cannot pass on the example dataset.
- Made the Rust workspace clean under `clippy -D warnings`.

This build is not declared Production Ready until the full acceptance and Codex benchmark programs pass.

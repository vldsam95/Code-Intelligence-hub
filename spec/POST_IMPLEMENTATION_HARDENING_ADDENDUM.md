# Code Intelligence Platform — Post-Implementation Hardening Addendum

**Version:** 1.0
**Date:** 2026-08-17
**Status:** Binding supplemental contract. The main specification takes precedence; ambiguity is resolved in favor of stricter identity, freshness and no-false-confidence guarantees.

## 1. Activation is opt-in

- Installing a Worker/MCP/skill on a host does not enable a project.
- Without an enabled `.code-intel/project.yaml`, status is `CODE_INTELLIGENCE_NOT_ENABLED`.
- No automatic `init`, indexing, Hub registration, gate, runtime or data-flow runs without a marker or explicit onboarding request.
- A project with only an LLM Wiki remains Wiki-only.
- The skill performs a lightweight marker check and exits immediately when disabled.

## 2. A single automatic onboarding flow

The user's request, “Add this project to Code Intelligence,” triggers:

```text
verify root → codeintel init → stable local project_id → manifest/policy
→ language/framework/check/Wiki discovery → source inventory/revision
→ local graph → automatic Hub Project+Instance registration
→ revision/health/capability sync
```

The normal workflow requires no manual Create Project action, UUID copying or instance creation in the Hub.

## 3. Registration is an idempotent upsert

- The same `project_id` returns the same logical project.
- The same `instance_id` is updated, not duplicated.
- An existing project receives a new instance only when its physical identity differs.
- Retrying after a timeout creates no duplicates.
- The payload includes identity, host/path, revision, capabilities, Worker/schema versions, index state and Wiki state.
- Registration never includes full source or raw graphs.

## 4. Offline-first

A Hub outage does not fail local initialization:

```text
local manifest/index/MCP/review/gate = AVAILABLE
hub_registration = PENDING
durable spool = persisted across Worker restart and reboot
automatic bounded backoff retry = enabled
```

When the Hub returns, the Worker registers the project and instance, sends the current revision and synchronizes historical evidence by revision. An old initial revision is not marked current if the source has changed.

## 5. Wiki binding

- modes: `none`, `local`, `remote`.
- Default: `none`, without degrading core features.
- One unambiguous local candidate may be bound; ambiguity stays `UNRESOLVED`.
- Remote binding requires explicit identity and credentials.
- A Wiki outage produces `UNAVAILABLE/PENDING` and does not change the quality gate.
- Code Intelligence owns live technical state; Wiki owns durable rationale/knowledge.
- Raw graphs, volatile callers and traces are never copied into the Wiki.

## 6. Session startup

For an enabled project:

```text
marker → open_project → short context → freshness/index check
→ load bound Wiki context when relevant → normal task workflow
```

After setup, no repeated prompt is required to activate Code Intelligence or the Wiki. Without a marker, continue normal/Wiki-only work.

## 7. Identity, move, fork and recovery

- `project_id` is created locally and retained in the manifest even while the Hub is offline.
- Moving `.code-intel/` preserves the logical project and creates a new instance.
- Concurrent `init` commands converge on one UUID through an advisory lock.
- A missing manifest at a known path returns `RECOVERY_REQUIRED`; no substitute identity is created.
- Recovery uses a single-use rebind token.
- A new fork is created only through an explicit `codeintel fork-project` command.

## 8. States and observability

The Hub and CLI explicitly display:

```text
registration: REGISTERED | REGISTRATION_PENDING | RECOVERY_REQUIRED
sync: CURRENT | SYNC_PENDING | OFFLINE
currentness: CURRENT | STALE | UNVERIFIED_CHANGE
wiki: BOUND | NONE | UNRESOLVED | UNAVAILABLE
```

`open_project`, `project_status` and `doctor` include project/instance identity, revision, index, registration, synchronization, spool and Wiki status.

## 9. Installer and Agent UX

- The installer detects Codex, Claude, Hermes and generic MCP clients and updates only product-owned entries.
- Installation and updates are idempotent and preserve existing Worker identity/configuration.
- Skills/MCP are installed for the selected agent account, including on multi-user hosts.
- The Hub provides a one-line HTTPS installer and a Complete Agent Kit ZIP from the same signed release.
- The ZIP includes Linux amd64/arm64 binaries, an offline installer, a Windows/WSL2 wrapper, checksums, a signed manifest, adapters, a skill, documentation and a universal prompt.
- The Hub provides a prepared text token guide. Secrets never enter logs, URLs or Git, and support Rotate/Revoke.

## 10. On-demand graph visualization

- Visualization is built only on an owner's request and while the Worker is online.
- The Worker returns bounded nodes/edges without source bodies.
- The Hub retains only a temporary payload with expiration, `no-store` and consume-once delivery.
- No renderer, collector or raw-graph daemon runs continuously.
- Visualization is discovery evidence only, never proof of correctness or absence.

## 11. Acceptance scenarios

Automated coverage and an appropriate pilot are required for:

1. An unmarked project remains inactive.
2. A Wiki-only project does not activate Code Intelligence.
3. A Code Intelligence-only project completes the workflow without a Wiki.
4. Code Intelligence plus Wiki loads the correct binding.
5. Online initialization automatically creates the Hub project and instance.
6. Offline initialization succeeds, pending state survives restart and synchronization resumes after recovery.
7. Duplicate retries do not duplicate rows.
8. Concurrent initialization creates one UUID.
9. A move creates a new instance under the same project.
10. A lost manifest uses recovery.
11. A Wiki outage does not block review/gating.
12. An on-demand graph is bounded, contains no source and is consumed once.

## 12. Definition of Done

This addendum is complete only when online and offline scenarios pass end to end without manual Hub setup, repeated initialization or duplicate identities; a new session detects the marker and Wiki binding automatically; and every status remains revision-aware and avoids false confidence.

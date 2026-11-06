# Code Intelligence — Simple Access Model and Project Portability

**Version:** V1.2 binding addendum
**Date:** 2026-08-18
**Status:** Binding alongside the main specification and V1 decisions

## The simple contract

The normal user experience exposes only:

```text
HOST ACCESS
= Can this host create or link new projects in the Hub?

PROJECT ACCESS
= Can an agent access Hub content for this project?

LOCAL CODE INTELLIGENCE
= Fully functional without either credential.
```

A Host Install Token is short-lived and single-use. The Worker credential remains an internal machine credential
for heartbeats, reports and relay. There is no separate token per Codex, Claude, Hermes or session; client,
version and timestamp are stored as metadata only.

## Local first

Without a Hub or token, during a Hub outage or after revocation, the Worker and local MCP continue providing graphs,
retrieval, preflight, risk analysis, review, tests/smoke, runtime, data-flow, dead-code candidates and quality
gates. Hub failure never turns a local gate into FAIL.

## Host Access

- Created during enrollment or idempotent bootstrap of an existing Worker.
- Stored locally outside Git and never included in an agent prompt.
- Permits creation/linking/registration of new projects and instances.
- Rotation/revocation blocks new provisioning only.
- Existing-project synchronization continues through a separate machine credential.
- Disconnect Worker is a separate advanced action.

## Project Access

- One token, scoped to one project, for all authorized agents.
- Permits context/health/quality/history/findings/agent setup, session metadata, bounded notes,
  read-only live analysis and binding an instance with actual source access.
- Grants no admin access, access to another project, remote shell or filesystem writes on another host.
- Rotation immediately invalidates the old value; revocation affects Hub access only.
- Stored locally only in `.code-intel/local/project-access.token` or a secure equivalent.

## Portability

The following travel with source/Git: identity/lineage, manifest, policy, checks, skill/agent documentation,
bounded technical context and deterministic graph-rebuild metadata. `.code-intel/local/`, tokens,
credentials, graph SQLite databases, caches, traces, spool and locks never enter Git.

Git contains everything required to rebuild the graph, not the derived graph itself:

```text
clone source + .code-intel portable inputs
→ codeintel open .
→ current non-Git SourceRevision
→ local graph rebuild
→ CURRENT only when the revision matches
```

The same logical project is linked through existing Project Access. A new logical fork/clone receives a new `project_id`
through `codeintel fork-project`. Git URLs, directory names, domains or source hashes are insufficient for attachment.

## UI and installation

- Hosts displays Host Access, provisioning enable/disable controls and advanced Worker synchronization.
- Each project displays Project Access Reveal/Copy/Rotate/Revoke/Test, MCP/API endpoints, complete agent
  setup, a token-free Agent Kit, connected instances and recent agent activity.
- The installer supports `--install-token` and `--local-only`, installing the Worker, MCP and skills without automatic initialization.
- One universal prompt conditionally handles Hub-connected/local-only operation, init/open/bind/fork and Wiki
  none/local/remote.

## Unchanged invariants

Source is truth; the graph is derived evidence; empty results do not prove absence; runtime proves only the observed
workload; evidence is bound to an exact revision; `UNKNOWN` is valid; the Hub stores no full source; dead code
is never deleted automatically; no remote LLM, heavy graph database or always-on collector is required; and every significant workflow ends
with review, required verification and a quality gate.

## Mandatory acceptance criteria

1. Single-use Install Tokens and local-only installation work.
2. Revoked Host Access blocks new creation/linking, but not local initialization, existing local work or Worker synchronization.
3. One Project Access credential works for Codex and Claude and cannot access another project.
4. Rotate/Revoke take immediate effect in the Hub without breaking local Code Intelligence.
5. Client/time activity is recorded without per-agent tokens or secrets in audit records.
6. A clone contains portable files, not secrets or graph databases, and rebuilds without a Hub.
7. A move using Project Access retains the project; a fork receives a new identity; ambiguous identity is never guessed.
8. An offline Hub leaves local gates functional and retains pending synchronization by revision.
9. The Agent Kit contains no token; setup containing a token requires Reveal after reauthentication.
10. A Project Token grants no shell access; secret files use `0600`, their directory uses `0700`, and secrets never enter logs.
11. All graph/review/runtime/data-flow/gate/relay/Wiki regression tests continue passing.

# Project Onboarding and Hub Registration

## Opt-in

Workers, MCP and skills are installed per host, but a project is not managed until `.code-intel/project.yaml` contains `enabled: true` or the user explicitly requests onboarding. A project without a marker returns `CODE_INTELLIGENCE_NOT_ENABLED`; initialization, indexing, registration, runtime and quality gates never run automatically.

## Hub online

```text
explicit user intent → codeintel init → stable local UUID → policy/inventory/index
→ idempotent Project+Instance upsert → current revision/index report → Hub UI
```

No manual Create Project action or UUID copying is required. Retrying with the same `project_id`/`instance_id` returns the same entities.

## Hub offline

Local initialization, graphs, MCP, review and gates continue working. The registry and reports persist under `/var/lib/code-intelligence`. Status is `registration=PENDING`, `hub_sync=OFFLINE/PENDING`; the daemon retries with backoff, including after reboot. When the Hub returns, the Worker sends the current revision first, followed by historical reports grouped by revision.

## Wiki discovery

The default is `none`. Automatic local binding requires one unambiguous candidate (`LLM-WIKI`, `llm-wiki`, `.llm-wiki`). Multiple candidates return `UNRESOLVED`. Remote binding requires `wiki_id`; an outage returns `UNAVAILABLE` and does not change the quality gate.

## Move and recovery

- Move with `.code-intel/`: the same project, a new instance and a local rebuild.
- Lost manifest: create a Recovery Token in the Hub and run `recover-project`; Git URLs never establish an automatic identity match.
- Logical fork: `codeintel fork-project` explicitly creates a new UUID.

## Concurrency

`init` uses a short-lived file lock. Concurrent requests converge on one manifest and UUID; registration is a replay-safe upsert.

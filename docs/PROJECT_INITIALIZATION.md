# Initialize a project

```bash
cd /path/to/project
codeintel init
```

Run this only after an explicit opt-in request. There is no need to create a project in the Hub or copy a UUID.

The command detects languages and frameworks, suggests roots and ignore rules, checks existing commands and generates:

```text
.code-intel/project.yaml
.code-intel/policy.yaml
.code-intel/AGENT_POLICY.md
```

Defaults: `wiki.mode=none`, `workspace.mode=live`. One unambiguous local Wiki candidate may be bound; multiple candidates produce `UNRESOLVED`, never an arbitrary choice. An initial index is then built and a Git-independent source revision is calculated.

When the Hub is available, the project, instance, revision, capabilities and Wiki state are registered automatically and idempotently. If the Hub is unavailable, initialization succeeds locally with `Hub registration: PENDING`; state and reports remain in the spool and the Worker retries automatically.

Two concurrent `init` commands converge on the same `project_id`. If a known path has lost its manifest, the command stops with `RECOVERY_REQUIRED` rather than creating a duplicate identity.

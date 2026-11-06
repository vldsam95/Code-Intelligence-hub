# Practical guide: move a project to another server

## Goal

Preserve the project ID and history, create a new instance and rebuild the graph locally.

## Your steps

1. Transfer the entire source directory, including `.code-intel/`.
2. Install a Worker on the new server if needed.
3. Open an agent at the new path and paste the prompt.

## Complete agent prompt

```text
The project has moved to <NEW_PROJECT_PATH> on a new host. Reconnect it to Code Intelligence without creating a new logical project.

1. Confirm that .code-intel/project.yaml, policy.yaml and AGENT_POLICY.md exist.
2. Read project_id from the manifest. Do not change it or match identity by Git URL.
3. Run codeintel open <NEW_PROJECT_PATH>, not codeintel init.
4. Allow the Worker to create a new ProjectInstance, calculate SourceRevision and rebuild derived indexes locally.
5. Verify that the Hub preserves existing history and displays the new instance; the old instance may become Offline/STALE.
6. Confirm that Worker credentials, graph databases, runtime traces and caches were not copied from the old host.
7. Run codeintel status and doctor; report any identity conflict.
8. Stop if the manifest is missing. Request a Recovery Token from the Hub and use recover-project; do not run a fresh init.

Report the preserved project_id, new instance_id, revision, index state, registration/synchronization and warnings.
```

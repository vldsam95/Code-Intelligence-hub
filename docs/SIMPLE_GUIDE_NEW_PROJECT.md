# Practical guide: enable Code Intelligence for a project

## Goal

Explicitly enable Code Intelligence for one project. No manual project creation in the Hub is required.

## Your steps

1. Start an agent inside the project directory.
2. Paste the following prompt and replace `<PROJECT_PATH>`.
3. Wait for the result. If the Hub is offline, `PENDING` is expected and the Worker will synchronize automatically.

## Complete agent prompt

```text
Enable Code Intelligence for the project at <PROJECT_PATH>. This is an explicit opt-in request.

1. Confirm the correct source root and check for a lost manifest requiring recover-project.
2. Run codeintel init on the path. Do not manually create a project in the Hub or invent project_id.
3. Allow init to detect languages, frameworks, source roots, ignores and verification commands. Do not invent smoke assertions.
4. The Wiki is optional and defaults to none. One unambiguous candidate may be proposed for binding; leave ambiguity UNRESOLVED.
5. Confirm that .code-intel/ contains portable inputs only: project.yaml, policy.yaml and AGENT_POLICY.md. Never store secrets or graph databases there.
6. Verify the initial source revision and CURRENT index, or report an explicit gap.
7. Verify automatic project and instance registration with the Hub. If the Hub is offline, local init must succeed and registration must remain durably PENDING in the spool.
8. Run codeintel status and codeintel doctor.

Report project_id, instance_id, revision, index state, Hub registration/synchronization, Wiki mode/binding, capabilities and unknowns.
```

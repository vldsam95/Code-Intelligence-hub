# Move a project between hosts

Transfer the code together with `.code-intel/`, install a Worker on the new host and run:

```bash
codeintel open /path/to/moved-project
```

The same `project_id` is retained and a new `ProjectInstance` is created. Graphs, caches and runtime data are rebuilt locally. Changing the Git remote or working without Git does not change identity.

Do not create another project in the Hub. The Worker automatically upserts the same `project_id`, adds an instance for the new host, preserves existing history and allows the old instance to become Offline/STALE.

If the manifest is lost, create a rebind token in the Hub and run `codeintel recover-project --token ...`. A Git URL alone never establishes a final automatic match.

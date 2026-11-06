---
name: code-intelligence-project
description: Project-local Code Intelligence workflow for Code Intelligence Platform.
---

# Project Code Intelligence

1. Open this project through the local Code Intelligence MCP.
2. Source is canonical; graph output is discovery evidence only.
3. Hub is optional. Missing/revoked Hub access never disables local analysis.
4. Before significant edits run preflight and inspect the actual source.
5. After edits run review, required verification and Quality Gate for the exact revision.
6. Report the local Gate and Hub sync as separate results.
7. Never commit `.code-intel/local/`, graph databases, caches, traces or credentials.
8. For enabled security scenarios, load the installed conditional security reference, validate first,
   run only a predeclared matrix against an explicitly authorized lab/staging origin, and correlate
   the resulting evidence to exact graph route symbols before verification when required.
9. Security evidence remains local and contains no response bodies or credential values.

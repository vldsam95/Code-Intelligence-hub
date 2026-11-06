# Practical guide: connect a project to an LLM Wiki

## Goal

Connect durable Wiki knowledge without treating it as source-code truth or blocking the quality gate during a Wiki outage.

## Your steps

1. Confirm that Code Intelligence is already enabled for the project.
2. Identify whether the Wiki is local or remote and obtain its exact ID/path.
3. Give the agent the following prompt.

## Complete agent prompt

```text
Connect the current Code Intelligence project to an LLM Wiki.

Wiki mode: <none|local|remote>
Wiki ID or local path: <WIKI_ID_OR_PATH>

1. Do not change project_id or rebuild the graph merely because of a binding change.
2. Confirm an explicit Code Intelligence capability in the Wiki; do not apply Code Intelligence prompts to a Wiki that is not a code project.
3. Code Intelligence/current source maintain live technical truth: symbols, callers, impact, review, tests, runtime and quality gates.
4. The Wiki stores only durable knowledge: rationale, decisions, invariants, integrations, operational constraints and meaningful verified results.
5. Never copy raw graphs, caller lists, traces or volatile relationships into the Wiki.
6. If the Wiki is unavailable, continue code work. Mark Wiki context UNAVAILABLE/update PENDING; do not turn the quality gate into FAIL.
7. Update the Wiki only after a successful quality gate and only when there is genuine durable knowledge.
8. Check binding status and explicitly report NONE/BOUND/UNRESOLVED/UNAVAILABLE.

Report the Code Intelligence project_id, Wiki mode, binding state, connector health and what each system will store.
```

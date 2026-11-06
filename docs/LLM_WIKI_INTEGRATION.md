# LLM‑Wiki Integration

The Wiki is optional. The three valid modes are `none`, `local` and `remote`; the default is `none`.

Code Intelligence explains current code behavior, changes, verification and quality. The Wiki preserves rationale, decisions, invariants and durable knowledge.

A Wiki outage does not block the quality gate. After a successful gate, an agent updates the Wiki only when there is durable knowledge; raw callers, graph dumps and temporary traces are not canonical knowledge.

During `init`, similar names alone never establish a binding. One unambiguous local candidate may be bound; ambiguity remains `UNRESOLVED`. Remote binding requires an explicit Wiki ID and credential. A project with only a Wiki and no Code Intelligence marker continues with the Wiki workflow alone.

When both systems are enabled, agents open current source/Code Intelligence first, then retrieve durable context from the Wiki. The live graph stays local; only durable conclusions verified after a successful gate are sent to the Wiki. If the Wiki is offline, code work continues and the Wiki update is marked `PENDING/UNAVAILABLE`.

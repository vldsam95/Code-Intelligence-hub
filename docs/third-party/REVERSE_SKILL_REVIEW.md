# Review record: reverse-skill

- Upstream: `https://github.com/zhaoxuya520/reverse-skill`
- Reviewed commit: `7e2097fd90d25c2f976f6eba26d6c00aa88051df`
- Review date: 2026-09-09
- Method: shallow read-only clone; scripts, bootstrap and payloads were not executed.

## Concepts retained

The implementation independently applies a small set of defensive design ideas: an explicit
authorization/scope gate, offline/lab/authorized network profiles, evidence bound to a finding and
source revision, SHA-256 evidence integrity, independent static/runtime corroboration, and pinned
supply-chain review.

## Material rejected

No upstream executable, bootstrapper, MCP registration, dependency installer, payload corpus or skill
prompt is vendored. Code Intelligence does not adopt self-modifying knowledge, runtime web searches,
automatic tool installation, brute-force workflows, denial-of-service tests, proxy/IP rotation,
credential attacks, exploit payloads, jailbreak material or exfiltration recipes.

This is a conceptual security review, not an upstream dependency. The exact commit is retained in the
offline baseline so a future maintainer can deliberately compare a newer revision. Code Intelligence
never fetches that revision during project analysis.

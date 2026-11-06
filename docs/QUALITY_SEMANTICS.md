# Quality Semantics

The quality gate returns `PASS`, `PASS_WITH_WARNINGS`, `UNKNOWN` or `FAIL`, with a separate currentness status: `CURRENT`, `STALE` or `UNVERIFIED_CHANGE`.

`PASS` requires all mandatory evidence to match the exact current source revision, all required checks to finish, and no blocking finding or UNKNOWN. An unsupported analyzer, unavailable required runtime, source drift during analysis or a non-current graph produces `UNKNOWN`.

`PASS` means the checks and evidence required by policy passed for the current source revision. It is not a mathematical guarantee that all business logic is correct.

An empty graph result does not prove absence. Runtime analysis that did not observe a path returns `NOT_OBSERVED`, never `DOES_NOT_EXIST`.


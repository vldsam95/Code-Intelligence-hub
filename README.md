# Code Intelligence Platform

Released under the [MIT License](LICENSE). You may use, modify and redistribute this
project, including commercially, while retaining the copyright and license notice.
Credit: [vldsam95 / Code-Intelligence-hub](https://github.com/vldsam95/Code-Intelligence-hub).
Contributions are welcome under the same license. Dependency licenses and upstream
acknowledgements are listed in [third-party notices](THIRD_PARTY_NOTICES.md).

A unified platform for local code analysis, review, runtime evidence and quality gates for Codex CLI,
Claude Code, Hermes and any compatible MCP client. This monorepo contains the central Hub and
everything needed to install a Worker on a VPS or another computer.

The optional `security_scenarios` layer adds governed API checks for authorization, IDOR/BOLA,
replay, rate limiting and provider budgets. It is local-only, disabled by default, and requires
a predefined policy and documented lab/staging authorization. Runtime results are correlated with
a route and a bounded code path without returning source bodies; mismatches remain UNKNOWN. See `docs/SECURITY_SCENARIOS.md`.

The canonical implementation contracts are maintained in this repository:

- [`spec/CODE_INTELLIGENCE_FINAL_PRODUCTION_SPEC_V1.md`](spec/CODE_INTELLIGENCE_FINAL_PRODUCTION_SPEC_V1.md)
- [`spec/IMPLEMENTATION_DECISIONS_V1.md`](spec/IMPLEMENTATION_DECISIONS_V1.md)
- [`spec/POST_IMPLEMENTATION_HARDENING_ADDENDUM.md`](spec/POST_IMPLEMENTATION_HARDENING_ADDENDUM.md)
- [`spec/SIMPLE_ACCESS_AND_PORTABILITY_ADDENDUM_V1_2.md`](spec/SIMPLE_ACCESS_AND_PORTABILITY_ADDENDUM_V1_2.md)

## Repository structure

- `hub/` — FastAPI, SQLite, an English UI, Remote Hub API/MCP, tokens, documentation and downloads.
- `worker/` — Rust workspace: Worker daemon, CLI, local MCP, SQLite graph, semantic resolver and framework registry.
- `adapters/` — Idempotent setup for Codex, Claude, Hermes and generic MCP clients.
- `installers/` — Online and offline installation, Windows/WSL2 support, and remote-host update/uninstall tools.
- `deploy/` — Docker Compose and Nginx for the Hub; systemd for the Worker.
- `protocol/` — Versioned JSON Schemas for Hub–Worker communication.
- `spec/` — The original product requirements and V1 decisions, preserved in full.
- `docs/` — Documentation for users and agents, also available in the Hub.
- `fixtures/` — Acceptance fixtures that do not depend on Git.
- `benchmarks/` — A harness comparing Codex alone with Codex plus the platform.

Run the acceptance fixtures:

```bash
scripts/run-acceptance-fixtures.sh
```

The Hub provides **Prompts & Skills** with a universal prompt and a prepared token guide, plus **Downloads**
with a one-line installer and a signed Complete Agent Kit ZIP from the same release. Workers are installed per
host; projects are enabled only through an existing marker or an explicit `codeintel init` command.

The benchmark harness does not treat its example dataset as production evidence. A positive verdict requires
at least 20 real tasks with paired measurements. Security tasks also measure recall against findings defined
in advance, so the graph receives no credit merely for producing more tool calls or context.

## Run the Hub locally

```bash
cp .env.example .env
docker compose -f deploy/docker-compose/docker-compose.yml up -d --build
```

Keep secrets out of Git. Production requires a master key in an external file with `0600` permissions
and an Argon2id hash for the owner's password.

## Worker development

```bash
cd worker
cargo test --workspace
cargo run -p codeintel-cli -- init /path/to/project
cargo run -p codeintel-cli -- open /path/to/project
```

## Delivery status

Progress against the specification is recorded in [`IMPLEMENTATION_STATUS.md`](IMPLEMENTATION_STATUS.md).
A successful build or deployment does not establish production readiness. That declaration requires
all gates A–F, all acceptance tests and the mandatory benchmark.

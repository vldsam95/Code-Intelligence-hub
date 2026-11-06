# Acceptance fixtures

These fixtures are small, deterministic projects for contract testing, not a marketing benchmark.
Each has a portable manifest, no Git dependency and `wiki.mode=none` to verify that the core works independently.

- `python-fastapi` — Routes, import resolution, type annotations and route-safe dead-code behavior.
- `php-laravel` — Laravel routes, controllers, container/job markers and Composer metadata.
- `typescript-express-dynamic` — An Express route alongside dynamic dispatch that must remain `UNKNOWN`.

Run the fixtures:

```bash
scripts/run-acceptance-fixtures.sh
```

The run includes hardening fixtures: a project without a marker stays inactive, concurrent `init` commands
converge on one identity, and an unavailable Hub leaves the local index active with durable `PENDING` registration.

# ADR 0001 — Validation deployment before Production declaration

The owner requested immediate deployment, while the canonical specification explicitly forbids a
Production declaration before Gates A–F, 117 combined acceptance checks, and the Codex benchmark.

Decision: deploy the secured Hub and signed Worker distribution as a validation deployment, keep the
implementation status visible, and do not label v1.0.0 Production Ready until the remaining evidence
exists. This preserves the specification's false-confidence invariant while allowing real pilots.


# Token Management

The normal workflow has two concepts: **Host Access** permits a host to create/link projects, and one **Project Access** credential per project serves all authorized agents. Initial setup uses a single-use Host Install Token. Worker credentials, legacy integrations, Admin API and Wiki credentials remain internal or under Advanced.

Revoking Host Access blocks only new creation/linking; local work and existing-project synchronization continue. Rotating/revoking Project Access blocks agent access to that project's Hub only; local graph, review, runtime and quality gates continue working.

Tokens have at least 256 bits of entropy, are encrypted with AES-256-GCM and validated using HMAC. Secrets are displayed only on creation or through a secured Reveal action; single-use tokens cannot be revealed again.

Rotate invalidates the old value and creates a new one. Revoke takes effect without a restart. Token values, session IDs and encryption keys are never written to logs or audit records.

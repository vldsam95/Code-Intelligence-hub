# Security and Sessions

The owner password uses Argon2id. Browser sessions are server-side; only a hash of the 256-bit session ID is stored in the database. Cookies use `Secure`, `HttpOnly`, `SameSite=Lax` and `Path=/`.

All browser mutations require CSRF protection. Login is limited to five failures in 15 minutes per account and source IP. Token Reveal requires reauthentication and is audited without recording the secret.

The Hub is exposed only through TLS via Nginx, with HSTS, CSP, nosniff, no-referrer and frame-ancestors none.


# Gotchas

- [Open the acceptance stack by a LAN address only over https; localhost is the plain-http origin](acceptance-lan-address-needs-https.md) — when running the Playwright browser tests, or ACC_HOST in scripts/dev/acceptance-up.sh
- [GitHub runners can carry a newer libpq5 than the pinned PG client: pin the client, not libpq5](ci-runner-libpq5-is-newer-than-the-pg-client.md) — when changing install-tools.sh pg-client-18 or a rust-test job fails installing the PostgreSQL 18 client

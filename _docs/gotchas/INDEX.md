# Gotchas

- [Open the acceptance stack by a LAN address only over https; localhost is the plain-http origin](acceptance-lan-address-needs-https.md) — when running the Playwright browser tests, or ACC_HOST in scripts/dev/acceptance-up.sh
- [GitHub runners can carry a newer libpq5 than the pinned PG client: pin the client, not libpq5](ci-runner-libpq5-is-newer-than-the-pg-client.md) — when changing install-tools.sh pg-client-18 or a rust-test job fails installing the PostgreSQL 18 client
- [The checkout has a default ACL (other::---): files in a bind-mounted folder are unreadable for other uids](checkout-default-acl-hides-files-from-other-uids.md) — when a stack container of another uid shares files through e2e-results, or new files show 0660
- [A new workspace crate must join WORKSPACE_PACKAGES in server/tests/crate_graph.rs](new-workspace-crate-must-join-the-planned-package-list.md) — when adding, moving or renaming a crate in src/Cargo.toml members
- [`docker compose start` can answer 0 and leave the container exited; check its start time](compose-start-can-answer-0-without-starting.md) — when e2e-ctl starts a service after a kill, or a harness step runs `compose start`
- [The CI `mobile` job can fail with Maven Central 429 on a cold Gradle cache; that is not the change](mobile-ci-maven-central-429.md) — when the only red CI job is `mobile` with a 429 from Maven Central

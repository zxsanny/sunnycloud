---
title: GitHub runner images can carry a newer libpq5 than the pinned PostgreSQL client; pin the client, not libpq5
applies_when: changing scripts/ci/install-tools.sh pg-client-18, bumping PG_CLIENT_VERSION, or a rust-test job failing in "Install the PostgreSQL 18 client"
source: 2026-11-26, CI run 45822062152 (rust-test 1 and 5)
---

libpq5 is one package for every PostgreSQL major version. After PGDG shipped 19.0, some `ubuntu-24.04` runner images came with PGDG's libpq5 and libpq-dev 19.0 already installed, while others did not, so only some shards failed. Installing `libpq5=<the 18.x client version>` then means a downgrade that also removes libpq-dev. `apt-get --yes` refuses it with "Packages were downgraded and -y was used without --allow-downgrades" (exit 100).

The fix: pin `postgresql-client-18` exactly and leave libpq5 to apt, since the client only needs `libpq5 (>= 18.x)` and libpq stays backward compatible. Do not add `--allow-downgrades`.

To check a change on Ubuntu 24.04 locally: in `noble-decoder-env:latest` (run as root), install PGDG's libpq5 and libpq-dev first, then run `scripts/ci/install-tools.sh pg-client-18`.

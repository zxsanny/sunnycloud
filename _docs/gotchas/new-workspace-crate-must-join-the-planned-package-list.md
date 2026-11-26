---
title: A new workspace crate must be added to server/tests/crate_graph.rs WORKSPACE_PACKAGES, or CI's rust-test fails
applies_when: adding or moving a crate in src/Cargo.toml `members`, or renaming a package
source: 2026-11-26, CI runs 45852755315 and 45856907728 (rust-test 1), after sunnycloud-fake-decoder joined the workspace
---

`src/server/tests/crate_graph.rs` test `workspace_has_exactly_the_planned_packages_at_their_folders` compares every
workspace package and its folder with the `WORKSPACE_PACKAGES` table. A crate added to `members` without a row there
passes its own tests, clippy and every other crate's tests, and fails only in that server test, which a parity run of
the changed crates does not reach. Add the row (`("<package>", "<folder under src>")`) in the same change, and run
`cargo test -p sunnycloud-server --test crate_graph` before the push. Check `NO_SQLX` and the other lists in the same
file when the new crate belongs to the worker or the decoder side.

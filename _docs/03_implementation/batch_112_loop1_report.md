# Batch 112 (loop 1): hello capabilities and the self-check, AZ-3861 (roadmap item 4, slice 5)

## What changed

- `hello` answers the real library versions and build time of the image. They come from one file, `/usr/share/sunnycloud/build-facts.json`
  (written by the image build, proven by `check-decoder-image.sh`), read when the handler is made (`src/decoder/src/build_facts.rs`).
  A missing or invalid file answers `unknown` and build time 1 (the lowest the protocol accepts) and is logged; the NVENC fields stay
  off. IT-10-1 now compares the three versions with the tools' own output in the image (`vips --version`, `ffmpeg -version`, the
  `libheif.so.x.y.z` the image links): 8.18.7, 9.1, 1.23.7.
- `self_check` reports seven items, always in the same order and with the same names: `forbidden_targets_unreachable`,
  `environment_is_allowlist`, `no_blob_or_secret_mounts`, `not_root`, `no_capabilities`, `no_new_privs`, `root_filesystem_read_only`
  (`src/decoder/src/self_check.rs`, `self_check/network.rs`). The old `proc_status_readable` item is gone: a file that cannot be read makes
  the items that need it `ok = false`. The resolve-and-connect attempts run side by side under one 1.5 s deadline, shorter than the
  smallest watchdog (3 s in the tests); a target still pending counts as reachable.
- Container suite: IT-10-1 asserts the versions; IT-10-2 asserts the seven names and all `ok` on the production container with the five spec
  targets; new `it_10_2_each_variant_flips_only_its_own_item` starts five variants (`cap_add NET_RAW`, uid 0, writable root, `X_TEST=1`,
  the blob path bind-mounted) and asserts exactly one item flips in each, with the same names. The harness got `Decoder::start_variant`.

## Two findings

- `cap_add NET_RAW` left `CapEff` at 0: a runner that is not root starts with an empty effective set whatever the container adds, so the
  spec's "effective capabilities are 0" could not flip (the spec's own Risk 1). `no_capabilities` now needs the effective, permitted and bounding
  sets all 0 (production: all 0; with NET_RAW only the bounding set shows it). Spec and 10 description say so.
- The first container run failed `environment_is_allowlist` in production: the runner calls `PR_SET_DUMPABLE 0`, which makes its own
  `/proc/self/environ` unreadable to it. The environment is now read in-process (`std::env::vars_os`).

## Evidence

Host: 229 decoder tests pass (clippy `-D warnings` clean); 19 new unit tests (crafted status files, mount tables, environments, a real listener
for the network item). Container (image built from this tree, `sunnycloud-decoder:az3861`): IT-10-1, IT-10-2 production and the five variants pass;
`check-decoder-image.sh` passes. CI run 45133395977 (03e8ea0) passed all 25 jobs, the `decoder-suite` job included.
One `image_fidelity` case failed once in a full host run at 2 s (it takes 17 s); it passed alone and in two further full runs: not reproduced.
The first CI run (45130998088, 7c802c6) failed one job: `rust-test (6)`, `process::tests::exit_code_and_signal_are_typed_whatever_stderr_says`, a `/proc/<pid>/stat` that did not parse in the process guard, unrelated to this batch. Not reproduced (a local stress scan was too slow); the error now names the pid and the text (03e8ea0) and a leftover records it: `_process_leftovers/2026-11-23_proc-stat-unparsable-once.md`.

## Not covered

- `forbidden_targets_unreachable` has no container variant that can flip it (it would need a container with a network, and the suite hosts have none to rely on):
  its logic is unit-tested against a real listener and a closed port.
- AZ-3865 still has NOT RUN items: IT-10-20 corpus, IT-10-13b, real phone data.

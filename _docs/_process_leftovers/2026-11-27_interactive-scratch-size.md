# The test stack's interactive scratch tmpfs is 1 GiB; the 09 description fixes 512 MiB

**Timestamp**: 2026-11-27
**Found by**: AZ-4387 (IT-09-7 (e), the free-space check), batch 133
**Blocked action**: none; no case depends on the size. This is a harness follow-up.

## What happens

`tests/compose.test.yml:40-41` creates the `scratch_interactive` volume as a tmpfs with `o: "size=1g"`.
`_docs/02_document/components/09_worker_supervisor/description.md:100` makes 512 MiB (`size=536870912`) the one value
and says the earlier `size=1g` is withdrawn: 256 MiB photo input + two outputs of at most 32 MiB = 320 MiB, a 192 MiB
margin, and a full tmpfs leaves 1.5 GiB of the decoder's 2 GiB limit for vips. With 1 GiB the stack runs a
`decoder-interactive` that can hold 1 GiB of scratch pages against the same 2 GiB limit, so a memory case would see
less room than the release has.

## Proposed handling

Set `o: "size=536870912"` on `scratch_interactive` in `tests/compose.test.yml` and in the dev Compose file
`deploy/compose/compose.dev.yml:83`, which carries the same `size=1g`, then run the worker suite on H2
(`tests/run.sh --profile h2`, `E2E_PYTEST_ARGS=suites/worker`) and the media suite. Owner: the harness (E-20 worker
suite, AZ-3878, or AZ-3879 for the memory checks).

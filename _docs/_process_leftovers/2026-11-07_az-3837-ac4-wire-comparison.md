# Leftover: AZ-3837 AC-4 byte-for-byte wire comparison

kind: test
blocked action: test that the `contracts` copies of the job types serialise byte-for-byte like the `media-shared` types (including `ClaimedJob.content_hash` as lowercase hex)
reason: `src/contracts/src/media/model.rs` has no serde derives, and `media-shared` must not depend on `contracts` (AC-6); only `media-api` (`src/components/08_media_jobs`) sees both

Next step: add serde derives (with lowercase-hex `content_hash` and `FailDisposition` serialising as `Disposition` does) to `contracts/src/media/model.rs`, then a JSON comparison test in `src/components/08_media_jobs/tests/media/` using the golden strings of `src/media-shared/tests/wire_names.rs`. Needs the contracts owner's agreement (additive change to a shared crate). Jira stays at In Testing until it is done.

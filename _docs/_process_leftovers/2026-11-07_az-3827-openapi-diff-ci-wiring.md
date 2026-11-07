# Leftover: AZ-3827 CI job for the OpenAPI breaking-change gate

kind: ci
blocked action: run `cargo run -p sunnycloud-http-api --bin openapi_diff -- diff <old> <new>` in CI against the document of the last release tag
reason: `scripts/` and the CI workflow were outside the worker's paths, and the repository has no release tag yet; the gate must skip cleanly when no tag exists

Next step: add the CI step (old document from the release tag with `... -- print`, or a committed copy; skip when no tag), and record the `x-*` extension names in `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (open point 1). AZ-3827 stays at In Testing until the CI step exists.

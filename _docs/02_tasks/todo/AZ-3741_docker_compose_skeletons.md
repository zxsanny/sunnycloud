# Dockerfile skeletons, .dockerignore, development compose file and database role init script

**Task**: AZ-3741_docker_compose_skeletons
**Name**: Dockerfiles, dev compose and role init script
**Description**: Build the api, worker and decoder images (and the `-test` tags of api and worker) from the repository with digest-pinned bases, non-root users and the documented labels; provide the development compose stack and the PostgreSQL 18 init script that creates the two application roles
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: Bootstrap (containers)
**Tracker**: AZ-3741
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c11

## Problem

The `images` CI job, the test harness and every later integration test need images built the same way as releases (one canonical path), and every database test needs the two roles of ADR-014 created exactly as in an operator install. Until the binaries can serve (E-22), the images must still build and declare their real health checks without faking them.

## Outcome

- `api`, `worker` and `decoder` images build from the repository root with `--locked` and digest-pinned bases; `api` and `worker` also build as `-test` with the `test-clock` feature.
- A development compose stack starts PostgreSQL 18 with both roles, and declares the rest of the documented services.
- The role init script creates `sunnycloud_migrator` (owner of the schema) and `sunnycloud` (DML-only runtime role) on an empty data directory.

## Scope

### Included
- Three multi-stage Dockerfiles per `deployment/containerization.md`: Rust builder by digest with cargo-chef and `cargo build --release --locked -p <package> --features ${FEATURES:-}` with `SQLX_OFFLINE=true`; runtime bases by digest (distroless `cc-debian13:nonroot` for api and worker, `debian:trixie-slim` for the decoder); users 10001, 10002, 10003; entrypoints and default commands; declared health checks (`sunnycloud healthcheck`, `sunnycloud-worker healthcheck`, `decoder-runner health`); build args `FEATURES`, `VERSION`, `REVISION`; OCI labels (source, version, revision, `licenses = AGPL-3.0-only`, `org.sunnycloud.test-clock = true|false`).
- The decoder image in skeleton form: the runner binary only, no media toolchain.
- `.dockerignore` at the repository root with the AZ-3652 entry list.
- The development compose file: db (Postgres 18 by digest, volume at `/var/lib/postgresql`, port bound to `127.0.0.1:5432` only, init script mounted), caddy (`tls internal`), migrate, api and worker built with `FEATURES=test-clock`, both decoders, mock-idp; throw-away secrets read from the gitignored development secrets folder; short durations.
- The role init script for an empty data directory: creates both roles from secret files, makes the migrator the owner of the application schema, gives `sunnycloud` login without superuser, `BYPASSRLS`, `CREATEDB` or `CREATEROLE`; later grants come from migrations.
- `.env.example` beside the compose files with the operator block of `environment_strategy.md` verbatim.

### Excluded
- The web build stage of the api image (E-26) and the media build stages, `versions.env`, `ffmpeg.configure`, SBOM `components.cdx.json` and source offer (E-19).
- `check-decoder-image.sh`, image push, signing, scanning (release workflows, E-19 and later).
- The 404 check of the test-clock routes (E-21 once the routes exist).
- The test compose file (E-00/c08) and the release `compose.yml` and Caddyfile (release bundle).

## Acceptance Criteria

**AC-1: Images build from the repository (E-00 AC 5)**
Given a clean checkout
When the api, worker and decoder images and the `-test` variants of api and worker are built from the repository root
Then all five builds exit 0, every `FROM` names a digest, and the cargo step uses `--locked`

**AC-2: Users and labels**
Given the five built images
When their configuration is inspected
Then api runs as 10001, worker as 10002, decoder as 10003; each has the OCI source, version, revision and `AGPL-3.0-only` licence labels; `org.sunnycloud.test-clock` is `false` on release builds and `true` on `-test` builds

**AC-3: Build context excludes local and secret files**
Given a checkout containing a `.env`, a development secrets folder, `_docs/` and a `target/` directory
When an image is built
Then none of them is in the build context or in any image layer (NFT-SEC-38 step 1, image part)

**AC-4: Roles created as ADR-014 requires**
Given an empty PostgreSQL 18 data directory and the role secret files
When the db container starts for the first time
Then `sunnycloud_migrator` exists and owns the application schema, and `sunnycloud` can log in, is not a superuser, has no `BYPASSRLS`, `CREATEDB` or `CREATEROLE` and owns nothing; on a second start with the same data directory the script changes nothing

**AC-5: Development stack**
Given the development compose file
When it is validated and the db service is started
Then validation passes, PostgreSQL 18 listens only on `127.0.0.1:5432`, its data volume is mounted at `/var/lib/postgresql`, and both roles exist

**AC-6: Health checks declared, not faked**
Given the three images and the development compose file
When their health-check commands are read
Then they are exactly the documented binaries' health commands; no check is a command that always succeeds

## Non-Functional Requirements

**Performance**
- With a warm cache, the three images build in under 10 minutes on a GitHub-hosted runner (skeleton binaries only).

**Compatibility**
- amd64 only in v1.0 (containerization O-4); Docker Compose as named in the release notes.

**Reliability**
- Build output is not silenced.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Lint of every `FROM` line | Each carries `@sha256:` |
| AC-6 | Search of Dockerfiles and compose for always-true health checks | 0 hits |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout | Build of 5 images | All exit 0 | Performance |
| AC-2 | Built images | Inspect user and labels | Users 10001/10002/10003; labels as listed | — |
| AC-3 | Planted `.env` and secrets folder | Build context and layer listing | Neither present (NFT-SEC-38 step 1) | — |
| AC-4 | Empty data dir; then restart | Role attributes query as superuser | As listed; unchanged on restart | — |
| AC-5 | Dev compose | `config` validation and db start | Pass; port bound to loopback only | — |

## Constraints

- RESTRICT-30: bases by digest; ADR-014: two roles, init script creates roles only; ADR-009: the worker image has no database secret and no media libraries.
- Postgres 18 data at `/var/lib/postgresql`, not `/var/lib/postgresql/data` (containerization, F20).

## Risks & Mitigation

**Risk 1: Health checks fail until E-22**
- *Risk*: the api and worker containers report unhealthy because `healthcheck` does not exist yet.
- *Mitigation*: accepted and stated; the harness (E-00/c08) does not start them until their epics make them healthy.

## Runtime Completeness

- Named capability: the decoder image's media toolchain (ffmpeg, libvips, libheif).
- Production code that must exist: the media stages of the decoder image, built by E-19.
- Allowed here: a decoder image with the runner only, labelled as such.
- Unacceptable: a decoder image or check that reports media capability (for example a self-check that passes) without the media toolchain.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The init script must set the two role passwords, but containerization mounts only `postgres_password` into the db container; which secret files the script reads for the role passwords is not specified. | Deploy owner | open | Medium |
| `containerization.md` places the development compose file at the repository root; AZ-3652 places it under `deploy/compose/` (module-layout open point 8 allows either). This task follows AZ-3652. | E-00 owner | resolved | Low |
| `environment_strategy.md` operator block for `.env.example` is copied verbatim; any later key change there must be repeated here. | Deploy owner | accepted-risk | Low |

## Contract

This task has no new contract file; the interface its consumers (E-00/c08, E-00/c09, the pipeline tasks) rely on is `_docs/02_document/deployment/containerization.md` (images, users, labels, health checks, compose services) and `_docs/02_document/deployment/environment_strategy.md` (variables).

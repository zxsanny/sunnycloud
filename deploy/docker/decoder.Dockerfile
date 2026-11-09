# Build from the repository root: docker build -f deploy/docker/decoder.Dockerfile .
# Skeleton form: the runner binary only. The media stages (libheif, libvips, ffmpeg), versions.env,
# the SBOM and the source offer arrive with E-19; this image carries no media toolchain.
FROM rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 AS chef
# RUSTUP_TOOLCHAIN overrides src/rust-toolchain.toml, whose extra components and targets are not in the image
ENV RUSTUP_TOOLCHAIN=1.97.1 \
    SQLX_OFFLINE=true
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /build/src

FROM chef AS planner
COPY src/ ./
RUN cargo chef prepare --recipe-path /build/recipe.json

FROM chef AS builder
ARG FEATURES=
COPY --from=planner /build/recipe.json /build/recipe.json
RUN cargo chef cook --release --locked --recipe-path /build/recipe.json -p decoder-runner --features "${FEATURES:-}"
COPY src/ ./
RUN cargo build --release --locked -p decoder-runner --features "${FEATURES:-}"

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
ARG VERSION=0.0.0-dev
ARG REVISION=unknown
LABEL org.opencontainers.image.source="https://github.com/zxsanny/sunnycloud" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.sunnycloud.test-clock="false" \
      org.sunnycloud.media-toolchain="absent"
RUN groupadd --gid 10003 decoder \
 && useradd --uid 10003 --gid 10003 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin decoder
COPY --from=builder /build/src/target/release/decoder-runner /usr/local/bin/decoder-runner
USER 10003:10003
ENV VIPS_BLOCK_UNTRUSTED=1
ENTRYPOINT ["decoder-runner"]
HEALTHCHECK CMD ["decoder-runner", "health"]
STOPSIGNAL SIGTERM

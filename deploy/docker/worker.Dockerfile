# Build from the repository root: docker build -f deploy/docker/worker.Dockerfile .
# Release image: no build args. Test image: --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true
# Supervisor only: no database secret and no media libraries (ADR-009).
FROM rust:1.97.1-trixie@sha256:b1b3c9c0d921d7fa0a6d1f9ec7e4eab87f8c8ec97644c3d791450f131dec813f AS chef
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
ARG TEST_CLOCK=false
# The test-clock label takes TEST_CLOCK; refuse a build where it disagrees with the compiled feature
RUN case ",${FEATURES}," in \
      *,test-clock,*) expected=true ;; \
      *) expected=false ;; \
    esac; \
    if [ "${TEST_CLOCK}" != "${expected}" ]; then \
      echo "TEST_CLOCK=${TEST_CLOCK} does not match FEATURES=${FEATURES}" >&2; exit 1; \
    fi
COPY --from=planner /build/recipe.json /build/recipe.json
RUN cargo chef cook --release --locked --recipe-path /build/recipe.json -p sunnycloud-worker --features "${FEATURES:-}"
COPY src/ ./
RUN cargo build --release --locked -p sunnycloud-worker --features "${FEATURES:-}"

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1
ARG VERSION=0.0.0-dev
ARG REVISION=unknown
ARG TEST_CLOCK=false
LABEL org.opencontainers.image.source="https://github.com/zxsanny/sunnycloud" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.sunnycloud.test-clock="${TEST_CLOCK}"
COPY --from=builder /build/src/target/release/sunnycloud-worker /usr/local/bin/sunnycloud-worker
USER 10002:10002
EXPOSE 9090
ENTRYPOINT ["sunnycloud-worker"]
HEALTHCHECK CMD ["sunnycloud-worker", "healthcheck"]
STOPSIGNAL SIGTERM

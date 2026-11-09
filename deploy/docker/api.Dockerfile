# Build from the repository root: docker build -f deploy/docker/api.Dockerfile .
# Release image: no build args. Test image: --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true
# The web build stage arrives with E-26.
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
RUN cargo chef cook --release --locked --recipe-path /build/recipe.json -p sunnycloud-server --features "${FEATURES:-}"
COPY src/ ./
RUN cargo build --release --locked -p sunnycloud-server --features "${FEATURES:-}"

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1
ARG VERSION=0.0.0-dev
ARG REVISION=unknown
ARG TEST_CLOCK=false
LABEL org.opencontainers.image.source="https://github.com/zxsanny/sunnycloud" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.sunnycloud.test-clock="${TEST_CLOCK}"
COPY --from=builder /build/src/target/release/sunnycloud /usr/local/bin/sunnycloud
USER 10001:10001
EXPOSE 8080 8081 9090
ENTRYPOINT ["sunnycloud"]
CMD ["serve"]
HEALTHCHECK --start-period=120s CMD ["sunnycloud", "healthcheck"]
STOPSIGNAL SIGTERM

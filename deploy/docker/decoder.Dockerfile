# Build from the repository root: docker build -f deploy/docker/decoder.Dockerfile .
# Stages (containerization.md, Decoder): fetch (hash-verified tarballs of versions.env) -> build-media (libheif, libvips,
# ffmpeg into /opt/media) -> builder (Rust decoder-runner) -> runtime (/opt/media plus Debian runtime libraries only).
# The first build takes about 25 minutes (ffmpeg); the media layers are keyed on versions.env and ffmpeg.configure.
# Pass --build-arg BUILD_TIMESTAMP=<unix seconds> and BUILD_DATE=<yyyymmdd of it> for a real build (build-images.sh does);
# the defaults mark a local build with the 1970 placeholders.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS debian

FROM debian AS fetch
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY deploy/docker/versions.env /build/versions.env
# Every tarball must match its pinned SHA-256; `sha256sum -c` fails the stage (and the build) on one wrong character.
RUN set -eu; . /build/versions.env; mkdir /src; \
    fetch() { curl --fail --silent --show-error --location --retry 3 --output "/src/$1" "$2"; echo "$3  /src/$1" | sha256sum --check -; }; \
    fetch libheif.tar.gz "$LIBHEIF_URL" "$LIBHEIF_SHA256"; \
    fetch vips.tar.xz "$VIPS_URL" "$VIPS_SHA256"; \
    fetch ffmpeg.tar.xz "$FFMPEG_URL" "$FFMPEG_SHA256"

FROM debian AS build-media
RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install -y --no-install-recommends \
    build-essential cmake meson ninja-build nasm pkg-config python3 xz-utils ca-certificates \
    libde265-dev libdav1d-dev libx264-dev libzimg-dev libwebp-dev libjpeg-dev libpng-dev libexif-dev liblcms2-dev \
    libglib2.0-dev libexpat1-dev zlib1g-dev \
 && rm -rf /var/lib/apt/lists/*
ENV PKG_CONFIG_PATH=/opt/media/lib/pkgconfig
WORKDIR /build

# libheif: decode only. libde265 (HEVC, the iPhone HEIC) and dav1d (AV1) are linked in, not loaded as plugins, so
# there is no plugin directory to leave out; no encoder (x265, x264, AOM, rav1e, SVT), no examples, no uncompressed codec.
COPY --from=fetch /src/libheif.tar.gz /build/libheif.tar.gz
RUN mkdir libheif && tar -xzf libheif.tar.gz --strip-components=1 -C libheif \
 && cmake -S libheif -B libheif/build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/opt/media -DCMAKE_INSTALL_LIBDIR=lib -DBUILD_SHARED_LIBS=ON -DBUILD_TESTING=OFF \
    -DENABLE_PLUGIN_LOADING=OFF -DWITH_EXAMPLES=OFF -DWITH_GDK_PIXBUF=OFF -DWITH_LIBSHARPYUV=OFF \
    -DWITH_UNCOMPRESSED_CODEC=OFF -DWITH_REDUCED_VISIBILITY=ON \
    -DWITH_LIBDE265=ON -DWITH_DAV1D=ON \
    -DWITH_X265=OFF -DWITH_X264=OFF -DWITH_KVAZAAR=OFF -DWITH_OpenH264_DECODER=OFF -DWITH_AOM_DECODER=OFF \
    -DWITH_AOM_ENCODER=OFF -DWITH_RAV1E=OFF -DWITH_SvtEnc=OFF -DWITH_JPEG_DECODER=OFF -DWITH_JPEG_ENCODER=OFF \
    -DWITH_OpenJPEG_DECODER=OFF -DWITH_OpenJPEG_ENCODER=OFF -DWITH_OPENJPH_ENCODER=OFF -DWITH_FFMPEG_DECODER=OFF \
    -DWITH_VVDEC=OFF -DWITH_VVENC=OFF -DWITH_UVG266=OFF \
 && cmake --build libheif/build --parallel 8 \
 && cmake --install libheif/build

# libvips: loaders jpeg, png, webp, heif and gif (built-in nsgif) only; SVG, PDF, ImageMagick, OpenSlide, FITS, NIfTI,
# Matlab, OpenEXR, TIFF, JPEG XL, JPEG 2000, libraw and the optional ppm/analyze/radiance readers are compiled out.
COPY --from=fetch /src/vips.tar.xz /build/vips.tar.xz
RUN mkdir vips && tar -xJf vips.tar.xz --strip-components=1 -C vips \
 && meson setup vips/build vips --prefix=/opt/media --libdir=lib --buildtype=release \
    -Ddeprecated=false -Dexamples=false -Dcplusplus=false -Dmodules=disabled -Dintrospection=disabled -Dvapi=false \
    -Dexif=enabled -Djpeg=enabled -Dlcms=enabled -Dpng=enabled -Dwebp=enabled -Dzlib=enabled -Dheif=enabled -Dnsgif=true \
    -Dheif-module=disabled -Dcfitsio=disabled -Dcgif=disabled -Dfftw=disabled -Dfontconfig=disabled -Darchive=disabled \
    -Dimagequant=disabled -Duhdr=disabled -Djpeg-xl=disabled -Dmagick=disabled -Dmatio=disabled -Dnifti=disabled \
    -Dopenexr=disabled -Dopenjpeg=disabled -Dopenslide=disabled -Dhighway=disabled -Dorc=disabled \
    -Dpangocairo=disabled -Dpdfium=disabled -Dpoppler=disabled -Dquantizr=disabled -Draw=disabled -Drsvg=disabled \
    -Dspng=disabled -Dtiff=disabled -Dppm=false -Danalyze=false -Dradiance=false \
 && meson compile -C vips/build -j 8 \
 && meson install -C vips/build

# ffmpeg: licence and attack-surface trimming live in ffmpeg.configure; the demuxer allowlist is FFMPEG_DEMUXERS.
COPY --from=fetch /src/ffmpeg.tar.xz /build/ffmpeg.tar.xz
COPY deploy/docker/versions.env deploy/docker/ffmpeg.configure /build/
RUN mkdir ffmpeg && tar -xJf ffmpeg.tar.xz --strip-components=1 -C ffmpeg \
 && . /build/versions.env \
 && demuxers="$(echo "$FFMPEG_DEMUXERS" | tr ',' '\n' | sed 's/^/--enable-demuxer=/')" \
 && cd ffmpeg \
 && ./configure --prefix=/opt/media $(grep -v '^#' /build/ffmpeg.configure) $demuxers \
 && make -j8 \
 && make install

# Only what runs stays: no headers, pkg-config files, static libraries, manual pages or cmake files.
RUN rm -rf /opt/media/include /opt/media/lib/pkgconfig /opt/media/lib/cmake /opt/media/lib/*.a /opt/media/lib/*.la \
    /opt/media/share/man /opt/media/share/doc /opt/media/share/ffmpeg /opt/media/share/locale /opt/media/share/gtk-doc

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
COPY --from=planner /build/recipe.json /build/recipe.json
RUN cargo chef cook --release --locked --recipe-path /build/recipe.json -p decoder-runner --features "${FEATURES:-}"
COPY src/ ./
RUN cargo build --release --locked -p decoder-runner --features "${FEATURES:-}"

FROM debian AS runtime
# Runtime libraries only, from Debian (patched by dist-upgrade on every rebuild).
ARG RUNTIME_PACKAGES="libx264-164 libde265-0 libdav1d7 libwebp7 libwebpmux3 libwebpdemux2 libjpeg62-turbo libpng16-16t64 libexif12 libzimg2 liblcms2-2 libglib2.0-0t64 libexpat1"
RUN apt-get update \
 && apt-get dist-upgrade -y \
 && apt-get install -y --no-install-recommends ${RUNTIME_PACKAGES} \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build-media /opt/media /opt/media
RUN echo /opt/media/lib >/etc/ld.so.conf.d/sunnycloud-media.conf \
 && ldconfig \
 && for tool in ffmpeg ffprobe vips vipsheader vipsthumbnail; do \
      if LD_LIBRARY_PATH= ldd "/opt/media/bin/$tool" | grep 'not found'; then echo "$tool has an unresolved library" >&2; exit 1; fi; \
    done
RUN groupadd --gid 10003 decoder \
 && useradd --uid 10003 --gid 10003 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin decoder
COPY --from=builder /build/src/target/release/decoder-runner /usr/local/bin/decoder-runner

# From here the layers depend on the build arguments (version, revision, build time).
ARG VERSION=0.0.0-dev
ARG REVISION=unknown
ARG BUILD_TIMESTAMP=1
ARG BUILD_DATE=19700101
ARG SOURCE_OFFER_URL=https://github.com/zxsanny/sunnycloud/releases/download/v${VERSION}/decoder-sources-${VERSION}-${BUILD_DATE}.tar.zst
COPY deploy/docker/versions.env deploy/docker/media-metadata.sh /tmp/media/
RUN VERSION="${VERSION}" BUILD_TIMESTAMP="${BUILD_TIMESTAMP}" BUILD_DATE="${BUILD_DATE}" SOURCE_OFFER_URL="${SOURCE_OFFER_URL}" \
    /tmp/media/media-metadata.sh /tmp/media/versions.env ${RUNTIME_PACKAGES} \
 && rm -rf /tmp/media
LABEL org.opencontainers.image.source="https://github.com/zxsanny/sunnycloud" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.sunnycloud.source-offer="${SOURCE_OFFER_URL}" \
      org.sunnycloud.test-clock="false" \
      org.sunnycloud.media-toolchain="present"
USER 10003:10003
ENV PATH=/opt/media/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    VIPS_BLOCK_UNTRUSTED=1
ENTRYPOINT ["decoder-runner"]
HEALTHCHECK CMD ["decoder-runner", "health"]
STOPSIGNAL SIGTERM

# Lists the build context exactly as the image builds receive it (AC-3). Build from the repository root.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
COPY . /ctx

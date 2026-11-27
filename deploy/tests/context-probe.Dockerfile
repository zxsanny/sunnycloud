# Lists the build context exactly as the image builds receive it (AC-3). Build from the repository root.
FROM debian:trixie-slim@sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0
COPY . /ctx

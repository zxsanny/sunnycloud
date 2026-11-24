# The mock identity provider as a container of the black-box test stack (tests/compose.test.yml). Build from this directory.
# It approves any sign-in: it is attached to the isolated test networks only and never published.
FROM oven/bun:1.4.5-alpine@sha256:da816670775f4c2b4e990b6b1847cf5540dcdc721a42033dd331ce7a88ba6a3b
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --production
COPY --chown=bun:bun src ./src
USER bun
EXPOSE 9000
CMD ["bun", "src/main.ts"]

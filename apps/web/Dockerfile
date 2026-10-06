ARG BUN_IMAGE=oven/bun:1.4.2

FROM ${BUN_IMAGE} AS build
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY src ./src
COPY scripts ./scripts
RUN bun run build

FROM ${BUN_IMAGE} AS release
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --production
COPY --from=build --chown=bun:bun /app/dist ./dist
COPY --chown=bun:bun src ./src
ENV NODE_ENV=production
USER bun
ENTRYPOINT ["bun", "src/server/index.ts"]

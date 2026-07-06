# Bundled Memory Layer service image: builds the web UI and the release binary,
# then ships a slim runtime. Used by the top-level compose.yaml so a user gets a
# working stack (Postgres + pgvector + service + web UI) with one command.
#
# Multi-arch friendly: every base image is available for amd64 and arm64.

# --- Stage 1: build the browser UI (web/dist) ---
FROM node:20-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json* ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- Stage 2: build the release binary ---
FROM rust:1-bookworm AS build
RUN apt-get update \
  && apt-get install -y --no-install-recommends pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# pgvector >= 0.6 rejects an HNSW index on a dimensionless vector column, which
# is what migration 0004 creates on a fresh database. The repository migration
# is intentionally left unchanged (existing databases validate its checksum),
# and migration 0016 drops this legacy index anyway. For a fresh-DB image build
# we make the index dimension-safe so migrations apply cleanly on boot. This
# mirrors evals/docker/app-build-sequence/Dockerfile.
RUN sed -i 's/ON memory_chunks USING hnsw (embedding vector_cosine_ops);/ON memory_chunks USING hnsw ((embedding::vector(1536)) vector_cosine_ops) WHERE embedding IS NOT NULL AND vector_dims(embedding) = 1536;/' migrations/0004_pgvector.sql
RUN cargo build --release --bin memory

# --- Stage 3: runtime ---
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates postgresql-client \
  && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/memory /usr/local/bin/memory
COPY --from=web /web/dist /usr/share/memory-layer/web
COPY deploy/config.docker.toml /etc/memory-layer/config.toml
ENV MEMORY_LAYER_PROFILE=prod
ENV MEMORY_LAYER_CONFIG=/etc/memory-layer/config.toml
EXPOSE 4040
CMD ["memory", "service", "run"]

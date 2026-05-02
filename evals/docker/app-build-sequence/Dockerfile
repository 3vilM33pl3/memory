FROM rust:1-bookworm

RUN apt-get update \
  && apt-get install -y --no-install-recommends nodejs npm postgresql-client ca-certificates git \
  && npm install -g @openai/codex \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY . /workspace
# pgvector requires dimensions on HNSW indexes. Keep the repository migration
# unchanged for existing databases, but compile the Docker eval image with a
# fresh-DB-safe expression index.
RUN sed -i 's/ON memory_chunks USING hnsw (embedding vector_cosine_ops);/ON memory_chunks USING hnsw ((embedding::vector(1536)) vector_cosine_ops) WHERE embedding IS NOT NULL AND vector_dims(embedding) = 1536;/' migrations/0004_pgvector.sql
RUN cargo build --bin memory

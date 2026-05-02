FROM rust:1-bookworm

RUN apt-get update \
  && apt-get install -y --no-install-recommends nodejs npm postgresql-client ca-certificates git \
  && npm install -g @openai/codex \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY . /workspace
RUN cargo build --bin memory

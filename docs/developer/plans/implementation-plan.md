# Memory Layer Implementation Plan

## Table of Contents

- [Summary](#summary)
- [Delivery order](#delivery-order)
- [Acceptance](#acceptance)

## Summary

Build the project as a Rust monorepo with:
- `mem-service` backend
- `mem-cli` CLI
- PostgreSQL persistence and search
- repo-local Codex skill in `.agents/skills/memory-layer/`
- Debian/systemd packaging assets

## Delivery order

1. Workspace and shared contracts
2. PostgreSQL schema and migrations
3. Query path
4. Capture ingestion
5. Deterministic curation
6. Skill wrappers
7. Packaging

## Acceptance

- `mem-service` boots and runs migrations
- `memctl capture-task` stores a raw capture
- `memctl curate` creates canonical memory
- `memctl query` returns stored memory with provenance
- the shipped skill scripts call the CLI correctly

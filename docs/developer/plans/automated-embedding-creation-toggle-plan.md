# Automated Embedding Creation Toggle Plan

## Goal

Add a TUI-visible toggle that controls only automatic embedding creation, while
leaving explicit `reembed` and `reindex` commands available for manual backfill.

## Checklist

- [x] Add `embeddings.create_enabled` config support with default `true`.
- [x] Expose `create_enabled` in embedding backend API responses.
- [x] Add a service endpoint and client method to toggle automatic embedding creation.
- [x] Persist `create_enabled` changes in `[embeddings]` config.
- [x] Skip automatic embedding provider writes after curation and bundle import when disabled.
- [x] Keep explicit `reembed` and `reindex` commands unaffected by the toggle.
- [x] Add a TUI Embeddings tab `c` keybinding and status/header text for creation toggle.
- [x] Update docs to distinguish search activation from automatic embedding creation.
- [x] Add tests for config parsing, persistence, TUI behavior, and automatic creation guards where feasible.
- [x] Run relevant formatting, tests, and plan completion verification.

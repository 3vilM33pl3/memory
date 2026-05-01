# Embeddings Provider Toggle Plan

## Goal

Let the TUI Embeddings tab switch semantic embedding retrieval on and off by
toggling the selected provider, while preserving configured backends and existing
embedding rows.

## Checklist

- [x] Add backend deactivation support in the search registry and service API.
- [x] Persist the off state explicitly in `[embeddings]` config, while keeping existing configs enabled by default.
- [x] Wire the CLI API client to call the deactivation endpoint.
- [x] Update the TUI Embeddings tab so `Enter` toggles the selected provider on or off.
- [x] Update status/control text for toggle behavior.
- [x] Update user-facing docs for the new off state.
- [x] Add tests for registry/config/service/TUI deactivation behavior where feasible.
- [x] Run relevant formatting, tests, and plan completion verification.

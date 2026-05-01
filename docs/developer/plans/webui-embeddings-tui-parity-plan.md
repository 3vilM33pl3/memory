# Web UI Embeddings TUI Parity Plan

## Goal

Bring the Web UI Embeddings tab in sync with the TUI controls for semantic
search activation, automatic creation toggles, and selected-backend embedding
maintenance actions.

## Checklist

- [x] Add Web API client methods for deactivating embeddings and toggling per-backend automatic creation.
- [x] Add Web TypeScript fields for per-backend and global embedding creation state.
- [x] Preserve selected backend by name after refresh and prefer the active backend on first load.
- [x] Add shared Web handlers for search toggle, creation toggle, selected-backend reembed, and selected-backend reindex.
- [x] Refresh backend coverage and project overview after embedding operations complete.
- [x] Update the Web Embeddings tab summary, backend list, and detail controls to match TUI behavior.
- [x] Add Embeddings-tab keyboard shortcuts for refresh, search toggle, creation toggle, reembed, and reindex.
- [x] Run the Web build and diff checks.

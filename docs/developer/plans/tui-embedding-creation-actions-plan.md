# TUI Embedding Creation Actions Plan

## Goal

Allow the Embeddings tab to kick off explicit embedding creation for the
highlighted backend without leaving the TUI.

## Checklist

- [x] Add selected-backend support to TUI API client `reembed` and `reindex` calls.
- [x] Keep existing global TUI shortcuts using all configured backends.
- [x] Add Embeddings tab keybinding to create missing embeddings for the highlighted backend.
- [x] Add Embeddings tab keybinding for full reindex of all configured backends.
- [x] Run embedding creation and reindex actions in background tasks.
- [x] Show in-progress, success, and failure status messages in the Embeddings tab.
- [x] Refresh backend coverage after embedding creation or reindex completes.
- [x] Update Embeddings tab controls and user docs.
- [x] Add focused tests for completion status handling.
- [x] Run relevant formatting, clippy, and tests.

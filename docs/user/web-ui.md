# Browser UI

The browser UI is the web companion to the TUI. It is served by `mem-service` and uses the same backend APIs for memories, query, activities, watchers, embeddings, resume briefings, and curation review.

Open it while the service is running:

```text
http://127.0.0.1:4040/
```

When building from source, run:

```bash
npm --prefix web ci
npm --prefix web run build
cargo run --bin memory -- service run
```

## Tabs

The main tabs match the TUI order:

| Tab | What it shows |
| --- | --- |
| Memories | Canonical memories, filters, markdown-style canonical text, embeddings, sources, history, and related memories. |
| Agents | Local Codex/Claude sessions, with the current project sorted first, plus tokens, context pressure, child processes, ports, and rate limits. |
| Query | Cited answers, ranked memory results, lexical/semantic/graph timing, token usage, ranking explanations, and graph connections. |
| Activity | Persisted activity, token/duration metadata, get-up-to-speed briefings, and LLM audit/debug status. |
| Errors | Persisted diagnostics plus browser-session errors with explanations, fix hints, `memory doctor` hints, commands, and raw errors. |
| Project | Project-level counts, memory type/source breakdowns, embedding coverage, automation state, watcher status, and recent activity. |
| Review | Replacement proposals with policy, target/candidate detail, and approve/reject actions. |
| Watchers | Watcher presence, heartbeat state, owner agent/session, restart attempts, and recovery details. |
| Embeddings | Configured embedding backends, active search backend, automatic creation, coverage, re-embed, and reindex controls. |
| Resume | Re-entry briefing with checkpoint, current thread, next steps, recent changes, context memories, timeline, warnings, and commits. |

The web-only `Bundles` tool remains under the More menu for bundle export/import previews and transfer.

## Runtime Status

The status strip shows the same operational components as the TUI bottom bar:

- `Web`: browser UI version and restart-required state.
- `Service`: backend version, primary/relay role, and service identity.
- `Manager`: watcher-manager state, mode, tracked sessions, and warnings.
- `Watchers`: active/unhealthy watcher counts.
- `Skills`: repo-local Memory skill bundle version and status.

If the install or upgrade process wrote a restart marker, the Web component turns into a restart state so the user knows to reload the page or restart the running UI.

## Query Evidence

The Query tab is designed to explain how an answer was produced:

- The answer cites numbered returned memories.
- The timing breakdown separates browser roundtrip, lexical search, semantic search, graph retrieval, rerank/relation work, and answer generation.
- Per-result details show score components such as full-text scores, semantic similarity, relation boost, graph boost, tag/path matches, confidence, importance, and recency.
- Graph connections explain which file or symbol helped retrieve a memory. Answers still cite memories, not raw graph rows.

Use the query history with the up/down arrows in the query box. Restoring a previous query restores both the question and the results that belonged to it.

## Activity And Audit

The Activity tab can generate two kinds of handoff briefing:

- `Deterministic`: cheap, grounded summary from persisted activities and memories.
- `LLM briefing`: synthesized narrative when an LLM backend is configured.

The same tab can toggle LLM audit/debug logging. Enable it briefly when debugging prompts or provider behavior, then disable it again because audit events can include large prompt payloads.

## Help

Press `h` or use the Help button to open contextual help for the active tab. Press `h` again or `Esc` to return.

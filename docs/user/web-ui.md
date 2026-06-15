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
| Automations | Loop-engineering automation cards with effective mode, scope, risk, last run, next trigger, daily budget, outputs, and controls. |

The web-only `Automations` and `Bundles` tools remain under the More menu. `Automations` controls the shared loop control plane; `Bundles` handles memory export/import previews and transfer.

## Automations

The Automations tab is the browser control surface for loop engineering. It reads
registered loop definitions from `/v1/loops`, resolves effective settings for the
current project/repo root, and displays the latest loop run for each automation.

Each automation card shows:

- effective mode and whether it is disabled, blocked, paused, snoozed, or stopped by the global kill switch;
- scope, including whether the active setting is inherited or overridden at project/repo level;
- risk level, next supported trigger, daily budget, outputs, and the most recent run;
- a mode menu for enabling or changing the loop mode.

The detail pane adds policy capabilities, blocked reasons, pause/snooze expiry,
last output, and action buttons:

- `Enable`: writes an explicit project/repo override using the definition default mode.
- `Disable`: turns the automation off for the current scope.
- `Pause 1h`: pauses the automation temporarily.
- `Snooze 1d`: suppresses the automation until the next day.
- `Run now`: records a manual control-plane run. In the first loop-engineering
  slices this is a dry-run/control-plane record; real autonomous execution is
  intentionally separate.
- `Global stop`: toggles the shared kill switch for all loop automations.

Use repo root carefully. When a repo root is resolved, setting changes are stored
as repo-scoped overrides. Without a repo root they are stored as project-scoped
overrides.

Use `Load run` on an automation with a last run to inspect the run ledger. The
detail view shows trigger source/event/trust, run reason, version, effective
settings, policy gates, cost, output, trace records, linked memory proposals, and
linked approval requests. Redacted trace payloads stay hidden and failed or
blocked runs surface their diagnostic summary and blocked reasons.

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

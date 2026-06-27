# Browser UI

The browser UI is the web companion to the TUI. It is served by `mem-service` and uses the same backend APIs for memories, query, code graph inspection, activities, watchers, embeddings, resume briefings, and curation review.

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
| Graph | 3D WebGL graph explorer with code neighborhoods, optional memory provenance links, optional memory relationship edges, and typed node/edge detail. |
| Activity | Persisted activity, token/duration metadata, get-up-to-speed briefings, and LLM audit/debug status. |
| Errors | Persisted diagnostics plus browser-session errors with explanations, fix hints, `memory doctor` hints, commands, and raw errors. |
| Project | Project-level counts, memory type/source breakdowns, embedding coverage, automation state, watcher status, and recent activity. |
| Review | Replacement proposals with policy, target/candidate detail, and approve/reject actions. |
| Watchers | Watcher presence, heartbeat state, owner agent/session, restart attempts, and recovery details. |
| Skills | Repo-local Memory skills, version status, upgrade action, file paths, `SKILL.md` content, and repair controls. |
| Embeddings | Configured embedding backends, active search backend, automatic creation, coverage, re-embed, and reindex controls. |
| Resume | Re-entry briefing with checkpoint, current thread, next steps, recent changes, context memories, timeline, warnings, and commits. |
| Automations | Loop-engineering approval queue, automation cards, effective mode, scope, risk, last run, next trigger, daily budget, outputs, and controls. |

The web-only `Bundles` tool remains under the More menu for memory export/import
previews and transfer. `Automations` is a primary browser tab and also has a
read-oriented TUI tab.

## Automations

The Automations tab is the browser control surface for loop engineering. It reads
registered loop definitions from `/v1/loops`, resolves effective settings for the
current project/repo root, and displays the latest loop run for each automation.

The top approval queue is the human-in-the-loop review surface for risky loop
actions and memory proposal changes. Each pending request shows:

- the proposed action JSON, action type, risk reason, requester, reviewer, loop,
  and linked run;
- linked memory proposal candidate/evidence when the proposed action references
  a memory proposal;
- `Approve`, `Reject`, and `Save edit` controls. Save edit validates the edited
  JSON before sending it to the service.

Rejected approvals are recorded in the run trace and block a queued/running
linked run so an agent cannot continue through a rejected gate. Approved,
rejected, and edited decisions also update linked memory proposal status when
the proposed action names a proposal id.

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
settings, policy gates, context pack, cost, output, trace records, linked memory
proposals, and linked approval requests. The context pack section shows included
memory count, estimated tokens, repo instruction references, exclusions, stale or
contradictory memory flags, warnings, and the diff from the previous context
pack trace for that loop/project. Redacted trace payloads stay hidden and failed
or blocked runs surface their diagnostic summary and blocked reasons.

## Skills

The Skills tab is the full inventory view for repo-local Memory Layer skills.
It defaults to the `memory-layer` umbrella skill so the main health signal stays
quiet, but the filter can switch to the full Memory-owned bundle when you need
to inspect focused skills such as query, resume, remember, proposal review, or
plan execution.

The list shows each skill's version, freshness, upgrade action, source path, and
template path. Selecting a row opens the detail pane with the description,
installed path, source/template versions, and rendered `SKILL.md` content so you
can inspect exactly what an agent will read.

Use `Repair skills` when the repo-local bundle is missing, stale, unversioned, or
corrupt. The action uses the same repair path as `memory doctor --fix`: it
downloads the current GitHub skill bundle when available, falls back to the
installed template when offline, backs up replaced files, and only mutates
Memory-owned skill directories.

## Graph Explorer

The Graph tab is a WebGL-only 3D explorer with three independently visible
layers:

- `Code`: parser-backed code graph neighborhoods from the latest completed
  graph extraction run. This layer is on by default.
- `Provenance`: memory-to-source links derived from active memories and their
  source records, including file, symbol, commit, and provenance verification
  status when available. This layer is off by default.
- `Memory relationships`: active memory-to-memory relation edges such as
  `supports`, `supersedes`, `duplicates`, `depends_on`, and `related_to`. This
  layer is off by default.

The browser does not extract or mutate graph data. The code layer reads
`/v1/projects/{project}/graph`; the memory layers read the separate read-only
`/v1/projects/{project}/memory-graph` endpoint and then merge visible layers in
the browser.

Build or refresh the code graph with:

```bash
memory graph extract --project memory
```

The default view is a bounded neighborhood, not an unlimited whole-repository
render. Filters can seed the graph from text search, file path, symbol name, edge
kind, depth, node cap, and edge cap. The service enforces hard caps of depth `2`,
`1000` nodes, and `2000` edges. When results are capped, the tab shows the
truncation reason.

The layer checkboxes sit underneath the 3D scene. Hovering a checkbox or a
visible node/edge brightens that whole layer and dims the others. Toggling the
provenance or relationship layers only changes the browser view; it does not
change backend code graph filters.

Clicking a code node selects it. Shift-clicking a second code node switches the
current browser view to the nodes and edges that connect those two nodes through
any simple path in the loaded code graph. This is a local view mode; normal node
selection, clearing the selection, Back/Forward, or refreshing the graph exits
it. The Shift-click connection view applies only to the Code layer in this
release.

Clicking a memory node shows its memory type, confidence, importance, tags, and
memory id. Clicking a source node shows source type, file path, symbol, commit,
verification status, and source id. Clicking provenance or relationship edges
shows their endpoints and edge-specific metadata.

Query result graph connections and graph extraction activity details include
actions that open the Graph tab with matching file, symbol, edge kind, or run id
filters.

Browsers without WebGL show an unsupported state. There is no SVG or 2D fallback
because the graph view is intended to exercise the same WebGL minimum supported
surface in every client.

## Runtime Status

The status strip shows the same operational components as the TUI bottom bar:

- `Web`: browser UI version and restart-required state.
- `Service`: backend version, primary/relay role, and service identity.
- `Manager`: watcher-manager state, mode, tracked sessions, and warnings.
- `Watchers`: active/unhealthy watcher counts.
- `Skills`: compact repo-local skill health for the selected Skills filter.
  Click the component to open the Skills tab for the full inventory and repair
  controls.

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

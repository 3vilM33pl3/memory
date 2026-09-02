# Web UI

The Web UI is the browser companion to the TUI. It is served by the same local Memory Layer service and reads the same project data: memory, cited queries, code graph context, activity, watchers, proposals, skills, embeddings, and resume briefings.

Open the installed service at `http://127.0.0.1:4040/`. From a source checkout, start the isolated dev service with:

```bash
cargo run --bin memory -- service run
```

The dev service uses `http://127.0.0.1:4250/` and its header carries a **DEV MODE** badge. That badge means the browser is connected to the isolated source profile, not the packaged service.

## Navigation

The main bar keeps everyday inspection visible:

| Primary tab | Use it for |
| --- | --- |
| Memories | Browse canonical memory, provenance, history, and related context. |
| Agents | Inspect local coding-agent sessions and their runtime state. |
| Query | Ask a cited question and inspect ranking/timing evidence. |
| Graph | Explore code and memory relationships visually. |
| Activity | Inspect persisted work and build handoff briefings. |
| Errors | Read actionable diagnostics and fix hints. |
| Project | See project-wide health, counts, and recent activity. |
| Review | Approve or reject replacement proposals. |
| Watchers | Check watcher ownership, heartbeat, and recovery. |
| Skills | Inspect managed and visible agent skills. |
| Embeddings | Check backends, coverage, activation, and maintenance controls. |
| Resume | Re-enter interrupted work with the current briefing. |

The **More** menu holds the lower-frequency control surfaces:

| More item | Use it for |
| --- | --- |
| Automations | Review loop definitions, effective modes, runs, and approval queues. |
| Bundles | Preview, export, and import portable memory bundles. |
| Access | Administer Authentik-backed identity, service tokens, and project memberships. This is visible only to global administrators. |

Automations, Bundles, and Access are not primary tabs. That keeps the everyday browser path focused while preserving the deeper controls when you need them.

## Go deeper: Graph explorer

The Graph tab is a WebGL-only 3D explorer with three independently visible layers:

- **Code** shows parser-backed code neighborhoods and is visible by default.
- **Provenance** adds active-memory-to-source links, including available file, symbol, commit, and verification information. It is off by default.
- **Memory relationships** adds active memory relationships such as `supports`, `supersedes`, `duplicates`, `depends_on`, and `related_to`. It is off by default.

The browser only reads graph data. Build or refresh the code layer from the CLI:

```bash
memory graph extract --project <project-slug>
```

The initial view is a bounded neighborhood, not an unlimited repository render. You can filter it by text, file, symbol, edge kind, depth, node cap, and edge cap; the service caps depth at `2`, nodes at `1000`, and edges at `2000`, and reports truncation when a cap applies.

Hovering a layer control or a visible node/edge highlights that layer and dims the others. Toggling Provenance or Memory relationships changes only the browser view; it does not rewrite graph data or backend filters. Click a code node to inspect it. Shift-click a second code node to show connecting paths in the loaded **Code** layer; clear the selection, use Back/Forward, or refresh to leave that local connection view.

Memory and source selections expose their corresponding type, identifiers, metadata, and relationships. Browsers without WebGL show an unsupported state: there is no 2D fallback for this view.

## Work safely

- Query answers expose cited memories, confidence, diagnostics, and graph context. Treat `insufficient_evidence` as a reason to investigate, not a hidden answer.
- Browser identity uses an HttpOnly session; the browser does not download a service token. Service-token secrets are displayed once when created and should go into a secret manager.
- Changes to loops, replacement proposals, embeddings, and access control write state. Review scope and confirmation controls before applying them.

For terminal-oriented inspection, use the [TUI](https://www.memory-layer.dev/docs/tui). For service and login recovery, start with [Help](https://www.memory-layer.dev/docs/help) or [service diagnostics](https://www.memory-layer.dev/docs/reference/cli/service-health).

import type { Tab } from "../tabs";

const SHARED_CONTROLS = [
  "Click tabs to switch sections.",
  "`h` or Help opens/closes this help panel.",
  "`r` refreshes project state when focus is not in an input.",
];

const WEB_HELP: Record<Tab, { title: string; purpose: string; layout: string[]; controls: string[]; workflows: string[] }> = {
  memories: {
    title: "Memories Help",
    purpose: "Browse canonical project memory, inspect details, provenance, embeddings, history, and related memories.",
    layout: ["Left side filters and memory list.", "Right side selected memory detail with markdown-like canonical text.", "Embeddings, tags, sources, and relations are grouped in detail sections."],
    controls: ["/ focuses memory search.", ...SHARED_CONTROLS],
    workflows: ["Filter by type, status, text, or tag.", "Verify sources before relying on a memory.", "Use History before deleting or replacing important memory."],
  },
  agents: {
    title: "Agents Help",
    purpose: "Monitor detected local coding-agent sessions, context pressure, rate limits, ports, and active work.",
    layout: ["Current project sessions are listed first.", "Detail pane shows model, tokens, context usage, process, git, children, and rate limits."],
    controls: ["Click an agent row to inspect it.", ...SHARED_CONTROLS],
    workflows: ["Check which agent owns a project or watcher.", "Use context and rate-limit data before adding work to a busy session."],
  },
  query: {
    title: "Query Help",
    purpose: "Ask project memory questions and inspect answer evidence, citations, timing, ranking, and graph connections.",
    layout: ["Question panel shows answer and timing breakdown.", "Results list shows ranked memories.", "Detail pane explains why the selected memory matched."],
    controls: ["Enter submits the question.", "ArrowUp/ArrowDown in the query input restores previous queries and their results.", "Click a result to inspect ranking details."],
    workflows: ["Compare answer citations with returned memories.", "Use timing fields to locate slow lexical, semantic, graph, rerank, answer, or UI phases.", "Treat graph connections as retrieval explanations, not standalone answer citations."],
  },
  graph: {
    title: "Graph Help",
    purpose: "Explore the extracted code graph as a bounded 3D WebGL neighborhood.",
    layout: ["Filter toolbar controls the graph slice.", "WebGL scene shows nodes and resolved edges.", "Inspector shows selected node or edge metadata."],
    controls: ["Apply reloads the graph slice.", "Refresh reloads current filters.", ...SHARED_CONTROLS],
    workflows: ["Start with a file or symbol filter.", "Open graph links from query results or graph extraction activities.", "Run memory graph extract when no graph is available."],
  },
  activity: {
    title: "Activity Help",
    purpose: "Review persisted backend activity and generate get-up-to-speed briefings for handoff or interruption recovery.",
    layout: ["Top panel generates deterministic or LLM briefings and shows LLM audit/debug status.", "Left table lists activity with token and duration summaries.", "Right pane shows structured details."],
    controls: ["Use Deterministic or LLM briefing buttons.", "Use the LLM audit button briefly while debugging prompts.", ...SHARED_CONTROLS],
    workflows: ["Generate a briefing before handing work to a new agent.", "Inspect token and duration fields to understand cost and latency.", "Open query activities to inspect graph behavior and answer cost."],
  },
  errors: {
    title: "Errors Help",
    purpose: "Inspect persisted diagnostics and browser-session errors with explanations and suggested fixes.",
    layout: ["Left list shows time, severity, source, component, and summary.", "Right pane shows explanation, fix hints, doctor hints, commands, and raw error."],
    controls: ["Click an error row to inspect it.", ...SHARED_CONTROLS],
    workflows: ["Open this tab when the footer shows errors or an operation fails.", "Prefer memory doctor hints when shown.", "Use source/component to route fixes to service, watcher, manager, provider, database, or browser."],
  },
  project: {
    title: "Project Help",
    purpose: "Show high-level project health, memory counts, embedding/search state, recent activity, automation, and watcher status.",
    layout: ["Metric panels summarize the project.", "Breakdowns show memory types, source kinds, tags, files, and recent activity."],
    controls: ["Project and repo root fields at the top choose scope.", ...SHARED_CONTROLS],
    workflows: ["Start here for a health check.", "Use counts to spot missing memory, missing embeddings, or pending curation."],
  },
  review: {
    title: "Review Help",
    purpose: "Approve or reject replacement proposals so duplicate or superseded memories are curated safely.",
    layout: ["Proposal list on the left.", "Candidate/target detail and policy controls on the right."],
    controls: ["Approve or Reject selected proposals.", "Cycle policy when a repo root is resolved.", ...SHARED_CONTROLS],
    workflows: ["Approve only when the candidate is clearly better and provenance remains valid.", "Reject ambiguous matches that would lose context."],
  },
  watchers: {
    title: "Watchers Help",
    purpose: "Show watcher heartbeat state, agent ownership, restart attempts, and recovery behavior.",
    layout: ["Summary panel shows counts and stale threshold.", "Watcher cards show owner/session/pid, host service, heartbeat, and restarts."],
    controls: [...SHARED_CONTROLS],
    workflows: ["Use this tab when captures are not appearing.", "Check owner/session and stale heartbeat before restarting anything."],
  },
  skills: {
    title: "Skills Help",
    purpose: "Inspect and repair repo-local Memory Layer skills used by coding agents.",
    layout: ["Toolbar filters Memory Layer versus all focused skills and runs refresh or repair.", "Left list shows status, local/template versions, and repair action for each skill.", "Right pane shows path, template source, status detail, and SKILL.md content."],
    controls: ["Use Repair skills to install missing skills or replace stale Memory-owned skills.", ...SHARED_CONTROLS],
    workflows: ["Open this tab when the status strip reports stale or missing skills.", "Review a skill path and instructions before asking an agent to use it.", "Repair skills here or use memory doctor --fix / memory upgrade from a terminal."],
  },
  embeddings: {
    title: "Embeddings Help",
    purpose: "Inspect embedding backends, switch semantic search, compare coverage, and backfill missing vectors.",
    layout: ["Summary shows active backend and create state.", "Backend list shows readiness and coverage.", "Detail pane has activation, creation, reembed, and reindex controls."],
    controls: ["Enter toggles selected backend search.", "c toggles automatic creation.", "e creates embeddings.", "I reindexes.", ...SHARED_CONTROLS],
    workflows: ["Use Create embeddings for normal missing-vector backfill.", "Use Reindex when chunks need rebuilding.", "Switch active backend after both spaces are populated to compare retrieval."],
  },
  resume: {
    title: "Resume Help",
    purpose: "Get back into flow with checkpoint, current thread, next steps, recent changes, attention items, and durable context.",
    layout: ["Load button generates the briefing.", "Scrollable detail shows checkpoint, next actions, summaries, memories, timeline, warnings, and commits."],
    controls: ["Click Load resume to refresh context.", ...SHARED_CONTROLS],
    workflows: ["Open this after interruption or when handing off work.", "Use the next-step section as the immediate continuation point."],
  },
  automations: {
    title: "Automations Help",
    purpose: "Review and control loop-engineering automations from the same backend control plane used by the CLI and MCP tools.",
    layout: ["Toolbar shows refresh and global stop.", "Approval queue shows pending risky actions, proposed JSON, run metadata, requester/reviewer, and linked memory proposals.", "Left side lists automation cards with mode, scope, risk, budget, trigger, and last run.", "Right side shows selected automation policy, context pack, outputs, blocked reasons, run ledger, traces, approvals, and proposals."],
    controls: ["Use Approve, Reject, or Save edit in the approval queue after reviewing the proposed action.", "Use mode menus to enable or reconfigure a loop.", "Use Load run to inspect the latest run ledger and context pack diff.", "Use Disable, Pause, Snooze, and Run now from the detail pane.", ...SHARED_CONTROLS],
    workflows: ["Review memory proposal candidates and evidence before approving durable memory changes.", "Inspect context pack warnings for stale or contradictory memory before trusting a loop run.", "Keep high-risk loops in suggest-only or draft-output modes until reviewed.", "Use global stop before investigating unexpected loop activity.", "Check scope text before changing inherited settings."],
  },
  bundles: {
    title: "Bundles Help",
    purpose: "Export and import portable memory bundles from the browser.",
    layout: ["Left side previews/downloads exports.", "Right side previews/applies imports."],
    controls: ["Choose export options before preview or download.", "Choose a bundle file before preview or import.", ...SHARED_CONTROLS],
    workflows: ["Preview before exporting or importing.", "Include provenance fields only when the bundle audience should see them."],
  },
};

export function HelpPanel({ tab }: { tab: Tab }) {
  const help = WEB_HELP[tab] ?? WEB_HELP.memories;
  return (
    <section className="panel help-panel">
      <h2>{help.title}</h2>
      <div className="help-grid">
        <div>
          <h3>Purpose</h3>
          <p>{help.purpose}</p>
        </div>
        <div>
          <h3>Layout</h3>
          <ul>{help.layout.map((item) => <li key={item}>{item}</li>)}</ul>
        </div>
        <div>
          <h3>Controls</h3>
          <ul>{help.controls.map((item) => <li key={item}>{item}</li>)}</ul>
        </div>
        <div>
          <h3>Workflows</h3>
          <ul>{help.workflows.map((item) => <li key={item}>{item}</li>)}</ul>
        </div>
      </div>
    </section>
  );
}

export const demoTabs = [
  "memories",
  "agents",
  "query",
  "graph",
  "activity",
  "errors",
  "project",
  "review",
  "watchers",
  "skills",
  "embeddings",
  "resume",
  "automations",
  "bundles",
] as const;

export type DemoTab = (typeof demoTabs)[number];

export interface DemoMemory {
  id: string;
  summary: string;
  preview: string;
  canonicalText: string;
  type: string;
  status: "active" | "archived";
  confidence: number;
  importance: number;
  tags: string[];
  sources: Array<{ kind: string; path: string; excerpt: string }>;
  related: Array<{ relation: string; summary: string }>;
}

export interface DemoGraphNode {
  id: string;
  label: string;
  kind: "code" | "memory" | "source";
  group: string;
  detail: string;
}

export interface DemoGraphLink {
  id: string;
  source: string;
  target: string;
  kind: "code" | "attachment" | "memory" | "provenance";
  label: string;
}

export const demoSnapshot = {
  project: "memory",
  repoRoot: "/workspace/memory",
  version: "0.9.5",
  capturedAt: "2026-06-27T18:00:00Z",
  overview: {
    activeMemories: 184,
    captures: 942,
    recentMemories: 23,
    proposals: 4,
    embeddingChunks: 3186,
    watchers: 2,
    graphNodes: 846,
    graphEdges: 1421,
  },
  memories: [
    {
      id: "mem-graph-radius",
      summary: "Graph isolation radius stays client-side",
      preview: "The Graph tab keeps isolate_depth local and anchors traversal on the selected node.",
      canonicalText:
        "The browser Graph tab owns the isolate_connected and isolate_depth controls. The service graph request shape stays unchanged; the browser filters returned edges as an undirected graph and colors visible nodes by degree.",
      type: "implementation",
      status: "active",
      confidence: 0.93,
      importance: 4,
      tags: ["web", "graph", "ui"],
      sources: [
        {
          kind: "file",
          path: "web/src/features/graph/GraphTab.tsx",
          excerpt: "Client-side graph filtering and degree color logic live in the Graph tab.",
        },
      ],
      related: [
        { relation: "supports", summary: "Memory nodes connect to selected code anchors" },
      ],
    },
    {
      id: "mem-docs-site",
      summary: "Docs site deploys from docs-site on Vercel",
      preview: "The public website is a separate Next/Fumadocs app with docs-site as the project root.",
      canonicalText:
        "The Memory Layer website is deployed from the docs-site directory. Public docs live under /docs, static images live under docs-site/public/images, and app routes live under docs-site/app.",
      type: "documentation",
      status: "active",
      confidence: 0.9,
      importance: 3,
      tags: ["docs-site", "vercel"],
      sources: [
        {
          kind: "file",
          path: "docs-site/README.md",
          excerpt: "The site is intended for Vercel deployment with docs-site as the project root.",
        },
      ],
      related: [
        { relation: "related_to", summary: "Website Browser UI documentation" },
      ],
    },
    {
      id: "mem-loop-automation",
      summary: "Loop automations use approval-gated control paths",
      preview: "Automation runs can produce reports, approval requests, and memory proposals.",
      canonicalText:
        "Loop automation surfaces are designed for inspection and approval. High-risk actions remain gated, and run traces expose context packs, policy decisions, output summaries, and proposed memory changes.",
      type: "architecture",
      status: "active",
      confidence: 0.86,
      importance: 4,
      tags: ["automations", "safety"],
      sources: [
        {
          kind: "file",
          path: "docs-site/content/docs/automations.mdx",
          excerpt: "Built-in loops run through the local service control plane.",
        },
      ],
      related: [
        { relation: "depends_on", summary: "Review tab replacement proposals" },
      ],
    },
    {
      id: "mem-release-homebrew",
      summary: "Release workflow verifies GitHub and Homebrew artifacts",
      preview: "Release prep checks tags, packages, GitHub release assets, and the Homebrew formula.",
      canonicalText:
        "The release workflow publishes GitHub artifacts and refreshes the Homebrew formula. Verification includes checking the release, package checksums, and local install behavior before reporting completion.",
      type: "reference",
      status: "archived",
      confidence: 0.78,
      importance: 2,
      tags: ["release", "homebrew"],
      sources: [
        {
          kind: "note",
          path: "skills/memory-release-homebrew/SKILL.md",
          excerpt: "Use for release prep plus GitHub/Homebrew verification.",
        },
      ],
      related: [],
    },
    {
      id: "mem-auth-helper",
      summary: "Repo-local skills prefer the dev CLI",
      preview: "The Go skill helper resolves the source checkout before an installed memory binary.",
      canonicalText:
        "Repo-local Memory Layer skills call the checkout CLI with cargo when they run inside the Memory source tree. This keeps dev-mode helpers on the dev service and avoids mixing installed CLI tokens with the repo dev endpoint.",
      type: "implementation",
      status: "active",
      confidence: 0.91,
      importance: 4,
      tags: ["skills", "auth", "dev"],
      sources: [
        {
          kind: "file",
          path: ".agents/skills/memory-layer/scripts/main.go",
          excerpt: "The resolver checks the source Cargo.toml and crates/mem-cli manifest before falling back to PATH.",
        },
      ],
      related: [
        { relation: "fixes", summary: "401 invalid api token from mixed dev/prod services" },
      ],
    },
    {
      id: "mem-web-demo",
      summary: "Public Web UI demo is backend-free",
      preview: "The /demo route uses sanitized static data and browser-only interactions.",
      canonicalText:
        "The public website includes a self-contained Web UI demo. It does not call the local service, does not load tokens, and reports a status message for actions that need a backend.",
      type: "documentation",
      status: "active",
      confidence: 0.89,
      importance: 3,
      tags: ["demo", "docs-site", "web-ui"],
      sources: [
        {
          kind: "file",
          path: "docs-site/components/demo/demo-app.tsx",
          excerpt: "Backend-only operations update the demo status bar instead of calling a service endpoint.",
        },
      ],
      related: [
        { relation: "extends", summary: "Docs site deploys from docs-site on Vercel" },
      ],
    },
    {
      id: "mem-mcp-read-first",
      summary: "MCP adapter is read-first",
      preview: "The built-in MCP server exposes query, resume, overview, activities, and memory inspection tools.",
      canonicalText:
        "The built-in MCP server is a protocol adapter over the existing Memory service API. The v1 surface is read-first and does not expose remember, curate, archive, delete, reindex, or proposal approval tools.",
      type: "architecture",
      status: "active",
      confidence: 0.87,
      importance: 4,
      tags: ["mcp", "api", "read-only"],
      sources: [
        {
          kind: "file",
          path: "crates/mem-mcp/src/lib.rs",
          excerpt: "MCP tools call the Memory API client instead of touching persistence directly.",
        },
      ],
      related: [
        { relation: "depends_on", summary: "Service API token protects HTTP MCP" },
      ],
    },
    {
      id: "mem-refactor-type",
      summary: "Refactor memories describe non-functional code reshaping",
      preview: "The curator can classify refactors and update affected memories when code moves.",
      canonicalText:
        "Refactor memories record code structure changes that intentionally preserve behavior. They should invalidate or update older memories whose file paths, modules, or ownership claims changed during the refactor.",
      type: "refactor",
      status: "active",
      confidence: 0.84,
      importance: 4,
      tags: ["curation", "refactor", "memory-types"],
      sources: [
        {
          kind: "file",
          path: "docs/developer/architecture/memory-types.md",
          excerpt: "Refactor memories document code movement without intended functional change.",
        },
      ],
      related: [
        { relation: "updates", summary: "Repository route split memories" },
      ],
    },
    {
      id: "mem-tui-sync",
      summary: "TUI memory detail stays synced with selection",
      preview: "When the memories list changes, the detail pane follows the selected memory.",
      canonicalText:
        "The TUI memories tab keeps the detail pane aligned with the current selection after list updates, so newly added or filtered memories do not leave stale detail text on screen.",
      type: "implementation",
      status: "active",
      confidence: 0.86,
      importance: 3,
      tags: ["tui", "memories", "selection"],
      sources: [
        {
          kind: "file",
          path: "crates/mem-cli/src/tui/tabs/memories.rs",
          excerpt: "Selection and detail rendering are handled together in the Memories tab.",
        },
      ],
      related: [
        { relation: "supports", summary: "Browser UI memory detail selection" },
      ],
    },
    {
      id: "mem-doc-reference",
      summary: "Reference docs favor dense command tables",
      preview: "CLI reference pages explain command shape, when to use each command, and realistic examples.",
      canonicalText:
        "The docs-site reference section should be high-density and operational. CLI pages should include purpose, flags, examples, output expectations, and common failure modes rather than shallow command lists.",
      type: "documentation",
      status: "active",
      confidence: 0.82,
      importance: 3,
      tags: ["docs", "cli", "reference"],
      sources: [
        {
          kind: "file",
          path: "docs-site/content/docs/reference/cli/index.mdx",
          excerpt: "The CLI reference groups commands by workflow and explains output contracts.",
        },
      ],
      related: [
        { relation: "supports", summary: "AGENTS.md as repository table of contents" },
      ],
    },
    {
      id: "mem-code-graph",
      summary: "Code graph extraction stores files, symbols, and references",
      preview: "Repository graph data links code nodes, imports, callers, and memory provenance.",
      canonicalText:
        "The code graph stores file, symbol, reference, and dependency nodes. Query and Web UI graph views use these edges to connect memories to affected source areas and to explain why graph boosts applied.",
      type: "architecture",
      status: "active",
      confidence: 0.88,
      importance: 4,
      tags: ["graph", "repository", "provenance"],
      sources: [
        {
          kind: "file",
          path: "migrations/0015_code_graph.sql",
          excerpt: "Code graph tables persist graph extraction runs, nodes, edges, symbols, and evidence.",
        },
      ],
      related: [
        { relation: "supports", summary: "Graph-aware retrieval" },
      ],
    },
    {
      id: "mem-watchers-dev",
      summary: "Dev watcher state is project-scoped",
      preview: "The Memory repo dev profile uses project-local runtime paths and foreground cargo services.",
      canonicalText:
        "When running the Memory repo in dev mode, service and watcher state is scoped to the project dev runtime. A cargo-run service is foreground and should not be treated as a background packaged service.",
      type: "environment",
      status: "active",
      confidence: 0.85,
      importance: 3,
      tags: ["dev", "watchers", "service"],
      sources: [
        {
          kind: "file",
          path: ".mem/config.dev.toml",
          excerpt: "The dev profile binds to the project-specific 4250/4251 endpoints.",
        },
      ],
      related: [
        { relation: "related_to", summary: "Repo-local skills prefer the dev CLI" },
      ],
    },
  ] satisfies DemoMemory[],
  agents: [
    {
      id: "agent-codex-main",
      name: "Codex CLI",
      project: "memory",
      status: "active",
      tokens: "92k / 256k",
      contextPressure: "36%",
      session: "web-demo-build",
      childProcesses: ["next dev", "cargo run --bin memory service run"],
      ports: ["3001", "4040"],
      rateLimit: "normal",
    },
    {
      id: "agent-claude-docs",
      name: "Claude Code",
      project: "memory",
      status: "idle",
      tokens: "41k / 200k",
      contextPressure: "21%",
      session: "docs-reference-review",
      childProcesses: ["npm run build"],
      ports: [],
      rateLimit: "normal",
    },
    {
      id: "agent-docs-site",
      name: "Docs preview",
      project: "memory",
      status: "active",
      tokens: "n/a",
      contextPressure: "n/a",
      session: "next-dev-preview",
      childProcesses: ["next dev --port 3001"],
      ports: ["3001"],
      rateLimit: "n/a",
    },
    {
      id: "agent-loop-worker",
      name: "Loop worker",
      project: "memory",
      status: "paused",
      tokens: "17k / 128k",
      contextPressure: "13%",
      session: "loop-control-plane",
      childProcesses: ["memory loops run --dry-run"],
      ports: [],
      rateLimit: "low",
    },
  ],
  query: {
    question: "How does the graph tab decide what memory to show?",
    answer:
      "The Graph tab starts from the selected code node, finds matching memory source records, and expands through the memory graph by the selected degree. Direct attachments are shown as code-to-memory links in the browser.",
    confidence: 0.88,
    diagnostics: [
      "lexical 64 / 23 ms",
      "semantic 46 / 112 ms",
      "graph 41 / 18 ms",
      "returned 4",
      "answer 531 ms",
    ],
    results: [
      { memoryId: "mem-graph-radius", score: 35.77, match: "hybrid", cited: true },
      { memoryId: "mem-code-graph", score: 31.68, match: "graph", cited: true },
      { memoryId: "mem-web-demo", score: 23.44, match: "semantic", cited: true },
      { memoryId: "mem-docs-site", score: 19.14, match: "lexical", cited: false },
      { memoryId: "mem-refactor-type", score: 16.83, match: "relation", cited: false },
      { memoryId: "mem-loop-automation", score: 12.42, match: "semantic", cited: false },
      { memoryId: "mem-auth-helper", score: 8.51, match: "recent", cited: false },
    ],
  },
  graph: {
    nodes: [
      { id: "code-graph-tab", label: "GraphTab", kind: "code", group: "web", detail: "web/src/features/graph/GraphTab.tsx" },
      { id: "code-controller", label: "useGraphController", kind: "code", group: "web", detail: "web/src/features/graph/useGraphController.ts" },
      { id: "code-api", label: "getMemoryGraph", kind: "code", group: "api", detail: "web/src/api.ts" },
      { id: "code-demo", label: "WebUiDemoApp", kind: "code", group: "docs-site", detail: "docs-site/components/demo/demo-app.tsx" },
      { id: "code-demo-data", label: "demoSnapshot", kind: "code", group: "docs-site", detail: "docs-site/components/demo/demo-data.ts" },
      { id: "code-mcp", label: "MCP server", kind: "code", group: "api", detail: "crates/mem-mcp/src/lib.rs" },
      { id: "code-service-auth", label: "require_token", kind: "code", group: "api", detail: "crates/mem-service/src/auth.rs" },
      { id: "code-skill-helper", label: "skill helper", kind: "code", group: "skills", detail: ".agents/skills/memory-layer/scripts/main.go" },
      { id: "code-curator", label: "curator", kind: "code", group: "service", detail: "crates/mem-service/src/repository/handlers/curation.rs" },
      { id: "code-tui-memories", label: "TUI memories", kind: "code", group: "tui", detail: "crates/mem-cli/src/tui/tabs/memories.rs" },
      { id: "code-loop", label: "loop control", kind: "code", group: "service", detail: "crates/mem-service/src/repository/handlers/loops.rs" },
      { id: "code-embeddings", label: "embeddings", kind: "code", group: "search", detail: "crates/mem-search/src/lib.rs" },
      { id: "memory-radius", label: "Graph radius memory", kind: "memory", group: "memory", detail: "Client-side degree filtering" },
      { id: "memory-docs", label: "Docs site memory", kind: "memory", group: "memory", detail: "Vercel docs-site deployment" },
      { id: "memory-auth", label: "Auth helper memory", kind: "memory", group: "memory", detail: "Dev CLI resolver behavior" },
      { id: "memory-demo", label: "Web demo memory", kind: "memory", group: "memory", detail: "Backend-free public demo" },
      { id: "memory-mcp", label: "MCP read-first", kind: "memory", group: "memory", detail: "Read-only MCP tool surface" },
      { id: "memory-refactor", label: "Refactor type", kind: "memory", group: "memory", detail: "Non-functional code reshaping" },
      { id: "memory-tui", label: "TUI sync", kind: "memory", group: "memory", detail: "Memories detail pane selection" },
      { id: "memory-code-graph", label: "Code graph", kind: "memory", group: "memory", detail: "Files, symbols, references" },
      { id: "memory-loop", label: "Loop automation", kind: "memory", group: "memory", detail: "Approval-gated automation" },
      { id: "memory-doc-reference", label: "Dense reference docs", kind: "memory", group: "memory", detail: "High-density CLI docs" },
      { id: "memory-watchers-dev", label: "Dev watcher state", kind: "memory", group: "memory", detail: "Project-scoped dev runtime" },
      { id: "source-graph", label: "GraphTab.tsx source", kind: "source", group: "source", detail: "Verified file source" },
      { id: "source-demo", label: "Demo component source", kind: "source", group: "source", detail: "docs-site/components/demo/demo-app.tsx" },
      { id: "source-helper", label: "Helper source", kind: "source", group: "source", detail: ".agents/skills/memory-layer/scripts/main.go" },
      { id: "source-schema", label: "Graph schema", kind: "source", group: "source", detail: "migrations/0015_code_graph.sql" },
      { id: "source-docs", label: "CLI docs source", kind: "source", group: "source", detail: "docs-site/content/docs/reference/cli/index.mdx" },
    ] satisfies DemoGraphNode[],
    links: [
      { id: "code-a", source: "code-graph-tab", target: "code-controller", kind: "code", label: "uses" },
      { id: "code-b", source: "code-controller", target: "code-api", kind: "code", label: "fetches" },
      { id: "code-c", source: "code-demo", target: "code-demo-data", kind: "code", label: "renders" },
      { id: "code-d", source: "code-demo", target: "code-graph-tab", kind: "code", label: "mirrors feature" },
      { id: "code-e", source: "code-mcp", target: "code-api", kind: "code", label: "adapts service API" },
      { id: "code-f", source: "code-service-auth", target: "code-api", kind: "code", label: "protects writes" },
      { id: "code-g", source: "code-skill-helper", target: "code-service-auth", kind: "code", label: "sends token" },
      { id: "code-h", source: "code-curator", target: "code-loop", kind: "code", label: "curates proposals" },
      { id: "code-i", source: "code-embeddings", target: "code-api", kind: "code", label: "serves retrieval" },
      { id: "code-j", source: "code-tui-memories", target: "code-api", kind: "code", label: "reads memories" },
      { id: "attach-a", source: "code-graph-tab", target: "memory-radius", kind: "attachment", label: "attached memory" },
      { id: "attach-b", source: "code-demo", target: "memory-demo", kind: "attachment", label: "attached memory" },
      { id: "attach-c", source: "code-skill-helper", target: "memory-auth", kind: "attachment", label: "attached memory" },
      { id: "attach-d", source: "code-mcp", target: "memory-mcp", kind: "attachment", label: "attached memory" },
      { id: "attach-e", source: "code-curator", target: "memory-refactor", kind: "attachment", label: "attached memory" },
      { id: "attach-f", source: "code-tui-memories", target: "memory-tui", kind: "attachment", label: "attached memory" },
      { id: "attach-g", source: "code-loop", target: "memory-loop", kind: "attachment", label: "attached memory" },
      { id: "attach-h", source: "code-embeddings", target: "memory-code-graph", kind: "attachment", label: "attached memory" },
      { id: "attach-i", source: "code-demo-data", target: "memory-docs", kind: "attachment", label: "attached memory" },
      { id: "rel-a", source: "memory-radius", target: "memory-docs", kind: "memory", label: "related_to" },
      { id: "rel-b", source: "memory-radius", target: "memory-code-graph", kind: "memory", label: "uses graph" },
      { id: "rel-c", source: "memory-demo", target: "memory-docs", kind: "memory", label: "extends" },
      { id: "rel-d", source: "memory-auth", target: "memory-watchers-dev", kind: "memory", label: "related_to" },
      { id: "rel-e", source: "memory-refactor", target: "memory-code-graph", kind: "memory", label: "updates" },
      { id: "rel-f", source: "memory-loop", target: "memory-mcp", kind: "memory", label: "reads context" },
      { id: "rel-g", source: "memory-doc-reference", target: "memory-demo", kind: "memory", label: "documents" },
      { id: "prov-a", source: "memory-radius", target: "source-graph", kind: "provenance", label: "source evidence" },
      { id: "prov-b", source: "memory-demo", target: "source-demo", kind: "provenance", label: "source evidence" },
      { id: "prov-c", source: "memory-auth", target: "source-helper", kind: "provenance", label: "source evidence" },
      { id: "prov-d", source: "memory-code-graph", target: "source-schema", kind: "provenance", label: "source evidence" },
      { id: "prov-e", source: "memory-doc-reference", target: "source-docs", kind: "provenance", label: "source evidence" },
    ] satisfies DemoGraphLink[],
  },
  activities: [
    { kind: "query", summary: "Answered graph-memory question", duration: "682 ms", tokens: "1.8k" },
    { kind: "curation", summary: "Curated 7 captures into 3 memories", duration: "2.4 s", tokens: "4.2k" },
    { kind: "graph_extract", summary: "Extracted 846 graph nodes and 1421 edges", duration: "31 s", tokens: "n/a" },
    { kind: "checkpoint", summary: "Saved direct task checkpoint for docs-site demo work", duration: "139 ms", tokens: "n/a" },
    { kind: "watcher_health", summary: "Watcher manager reported one active project watcher", duration: "48 ms", tokens: "n/a" },
    { kind: "mcp", summary: "Listed read-only MCP tools for memory project", duration: "411 ms", tokens: "n/a" },
    { kind: "embedding", summary: "Reindexed 217 stale memory chunks", duration: "18 s", tokens: "n/a" },
    { kind: "bundle_export", summary: "Previewed memory bundle with relations and provenance", duration: "531 ms", tokens: "n/a" },
  ],
  errors: [
    {
      code: "service_auth_failed",
      severity: "error",
      message: "401 Unauthorized invalid api token",
      fix: "Run memory doctor and verify the service api token in local config.",
    },
    {
      code: "webgl_unavailable",
      severity: "warning",
      message: "Browser graph explorer requires WebGL.",
      fix: "Enable hardware acceleration or use a WebGL-capable browser.",
    },
    {
      code: "service_down",
      severity: "error",
      message: "Could not reach http://127.0.0.1:4250/healthz",
      fix: "Start the foreground dev service with cargo run --bin memory service run.",
    },
    {
      code: "stale_skill_bundle",
      severity: "warning",
      message: "Repo-local skills do not match the installed template.",
      fix: "Run memory doctor --repair-skills after reviewing the diff.",
    },
  ],
  proposals: [
    {
      id: "proposal-1",
      status: "pending",
      policy: "balanced",
      target: "Graph isolation radius stays client-side",
      candidate: "Graph isolation and memory radius controls are browser-only and do not change service graph request shape.",
      reason: "A new implementation memory overlaps and should update the older radius memory.",
    },
    {
      id: "proposal-2",
      status: "pending",
      policy: "conservative",
      target: "Docs site deploys from docs-site on Vercel",
      candidate: "The public website includes a backend-free demo at /demo.",
      reason: "New docs-site route extends the existing website deployment knowledge.",
    },
    {
      id: "proposal-3",
      status: "pending",
      policy: "balanced",
      target: "Repo-local skills prefer the dev CLI",
      candidate: "The helper should use the checkout CLI in source trees and only use PATH as a fallback.",
      reason: "A recent 401 fix refined how skill helpers resolve the Memory CLI.",
    },
    {
      id: "proposal-4",
      status: "pending",
      policy: "conservative",
      target: "Reference docs favor dense command tables",
      candidate: "CLI reference examples should show realistic flags, JSON output use, and recovery hints.",
      reason: "Docs review found shallow or stale reference examples.",
    },
  ],
  watchers: [
    { repo: "/workspace/memory", state: "healthy", heartbeat: "12s ago", owner: "manager", restarts: 0 },
    { repo: "/workspace/demo-project", state: "recovering", heartbeat: "2m ago", owner: "agent session", restarts: 1 },
    { repo: "/workspace/memory/docs-site", state: "healthy", heartbeat: "33s ago", owner: "preview", restarts: 0 },
    { repo: "/workspace/memory-loop", state: "paused", heartbeat: "18m ago", owner: "loop automation", restarts: 0 },
  ],
  skills: [
    { name: "memory-layer", status: "current", version: "0.9.5", path: ".agents/skills/memory-layer/SKILL.md" },
    { name: "memory-query-resume", status: "current", version: "0.9.5", path: ".agents/skills/memory-query-resume/SKILL.md" },
    { name: "memory-review-proposals", status: "outdated", version: "0.9.4", path: ".agents/skills/memory-review-proposals/SKILL.md" },
    { name: "memory-direct-task-start", status: "current", version: "0.9.5", path: ".agents/skills/memory-direct-task-start/SKILL.md" },
    { name: "memory-plan-execution", status: "current", version: "0.9.5", path: ".agents/skills/memory-plan-execution/SKILL.md" },
    { name: "memory-remember", status: "current", version: "0.9.5", path: ".agents/skills/memory-remember/SKILL.md" },
    { name: "memory-project-init", status: "current", version: "0.9.5", path: ".agents/skills/memory-project-init/SKILL.md" },
  ],
  embeddings: [
    { provider: "openai", model: "text-embedding-3-large", search: true, creation: true, fresh: 2860, stale: 109 },
    { provider: "ollama", model: "nomic-embed-text", search: false, creation: false, fresh: 217, stale: 0 },
    { provider: "openai", model: "text-embedding-3-small", search: false, creation: false, fresh: 642, stale: 42 },
    { provider: "local", model: "sqlite-keyword-index", search: true, creation: true, fresh: 1211, stale: 0 },
  ],
  resume: {
    briefing: "Continue the docs-site demo implementation. The current branch has recent Graph tab UI commits, repo-local skill helper auth was repaired, and the website deploys from docs-site.",
    next: ["Validate /demo in desktop and mobile widths", "Publish docs-site to Vercel", "Capture implementation memory after the graph renderer fix"],
    warnings: ["The public demo uses static data; search, curation, repair, and loop execution show status messages instead of mutating backend state."],
  },
  automations: [
    { name: "memory_hygiene", mode: "suggest_only", risk: "medium", state: "enabled", run: "succeeded", approvals: 1 },
    { name: "context_pack_refresh", mode: "observe", risk: "low", state: "enabled", run: "queued", approvals: 0 },
    { name: "ci_failure_triage", mode: "draft_output", risk: "medium", state: "paused", run: "blocked", approvals: 2 },
    { name: "stale_skill_repair", mode: "suggest_only", risk: "medium", state: "enabled", run: "waiting_for_approval", approvals: 1 },
    { name: "graph_reextract", mode: "draft_output", risk: "low", state: "enabled", run: "scheduled", approvals: 0 },
    { name: "proposal_review_assist", mode: "observe", risk: "low", state: "enabled", run: "succeeded", approvals: 0 },
  ],
  bundles: {
    options: ["active memories", "relations", "tags", "source paths", "activity digest", "graph neighborhood", "proposal previews"],
    exportPreview: "12 memories, 24 relations, 21 sources, 8 activity summaries, estimated 64 KB JSON bundle",
    importPreview: "5 new memories, 3 replacement proposals, 0 destructive changes, 2 provenance warnings",
  },
};

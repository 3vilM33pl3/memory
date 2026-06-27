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
      { memoryId: "mem-docs-site", score: 19.14, match: "lexical", cited: false },
      { memoryId: "mem-loop-automation", score: 12.42, match: "semantic", cited: false },
    ],
  },
  graph: {
    nodes: [
      { id: "code-graph-tab", label: "GraphTab", kind: "code", group: "web", detail: "web/src/features/graph/GraphTab.tsx" },
      { id: "code-controller", label: "useGraphController", kind: "code", group: "web", detail: "web/src/features/graph/useGraphController.ts" },
      { id: "code-api", label: "getMemoryGraph", kind: "code", group: "api", detail: "web/src/api.ts" },
      { id: "memory-radius", label: "Graph radius memory", kind: "memory", group: "memory", detail: "Client-side degree filtering" },
      { id: "memory-docs", label: "Docs site memory", kind: "memory", group: "memory", detail: "Vercel docs-site deployment" },
      { id: "source-graph", label: "GraphTab.tsx source", kind: "source", group: "source", detail: "Verified file source" },
    ] satisfies DemoGraphNode[],
    links: [
      { id: "code-a", source: "code-graph-tab", target: "code-controller", kind: "code", label: "uses" },
      { id: "code-b", source: "code-controller", target: "code-api", kind: "code", label: "fetches" },
      { id: "attach-a", source: "code-graph-tab", target: "memory-radius", kind: "attachment", label: "attached memory" },
      { id: "rel-a", source: "memory-radius", target: "memory-docs", kind: "memory", label: "related_to" },
      { id: "prov-a", source: "memory-radius", target: "source-graph", kind: "provenance", label: "source evidence" },
    ] satisfies DemoGraphLink[],
  },
  activities: [
    { kind: "query", summary: "Answered graph-memory question", duration: "682 ms", tokens: "1.8k" },
    { kind: "curation", summary: "Curated 7 captures into 3 memories", duration: "2.4 s", tokens: "4.2k" },
    { kind: "graph_extract", summary: "Extracted 846 graph nodes and 1421 edges", duration: "31 s", tokens: "n/a" },
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
  ],
  watchers: [
    { repo: "/workspace/memory", state: "healthy", heartbeat: "12s ago", owner: "manager", restarts: 0 },
    { repo: "/workspace/demo-project", state: "recovering", heartbeat: "2m ago", owner: "agent session", restarts: 1 },
  ],
  skills: [
    { name: "memory-layer", status: "current", version: "0.9.5", path: ".agents/skills/memory-layer/SKILL.md" },
    { name: "memory-query-resume", status: "current", version: "0.9.5", path: ".agents/skills/memory-query-resume/SKILL.md" },
    { name: "memory-review-proposals", status: "outdated", version: "0.9.4", path: ".agents/skills/memory-review-proposals/SKILL.md" },
  ],
  embeddings: [
    { provider: "openai", model: "text-embedding-3-large", search: true, creation: true, fresh: 2860, stale: 109 },
    { provider: "ollama", model: "nomic-embed-text", search: false, creation: false, fresh: 217, stale: 0 },
  ],
  resume: {
    briefing: "Continue the docs-site demo implementation. The current branch has recent Graph tab UI commits and the website deploys from docs-site.",
    next: ["Validate /demo in desktop and mobile widths", "Publish docs-site to Vercel", "Capture implementation memory after token repair"],
    warnings: ["Local service memory writes currently return 401 invalid api token."],
  },
  automations: [
    { name: "memory_hygiene", mode: "suggest_only", risk: "medium", state: "enabled", run: "succeeded", approvals: 1 },
    { name: "context_pack_refresh", mode: "observe", risk: "low", state: "enabled", run: "queued", approvals: 0 },
    { name: "ci_failure_triage", mode: "draft_output", risk: "medium", state: "paused", run: "blocked", approvals: 2 },
  ],
  bundles: {
    options: ["active memories", "relations", "tags", "source paths"],
    exportPreview: "4 memories, 6 relations, 9 sources, estimated 18 KB JSON bundle",
    importPreview: "2 new memories, 1 replacement proposal, 0 destructive changes",
  },
};

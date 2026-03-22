import { useEffect, useMemo, useRef, useState } from "react";
import {
  archiveProject,
  curate,
  deleteMemory,
  getHealth,
  getMemory,
  getMemories,
  getOverview,
  reindex,
  runQuery,
} from "./api";
import type {
  ActivityDetails,
  ActivityEvent,
  MemoryEntryResponse,
  MemoryStatus,
  MemoryType,
  ProjectMemoriesResponse,
  ProjectOverviewResponse,
  QueryResponse,
  StreamRequest,
  StreamResponse,
} from "./types";

type Tab = "memories" | "query" | "activity" | "project";

type MemoryTypeFilter = "all" | MemoryType;
type StatusFilter = "all" | MemoryStatus;

const EMPTY_OVERVIEW: ProjectOverviewResponse = {
  project: "memory",
  service_status: "unknown",
  database_status: "unknown",
  memory_entries_total: 0,
  active_memories: 0,
  archived_memories: 0,
  high_confidence_memories: 0,
  medium_confidence_memories: 0,
  low_confidence_memories: 0,
  recent_memories_7d: 0,
  recent_captures_7d: 0,
  raw_captures_total: 0,
  uncurated_raw_captures: 0,
  tasks_total: 0,
  sessions_total: 0,
  curation_runs_total: 0,
  last_memory_at: null,
  last_curation_at: null,
  last_capture_at: null,
  oldest_uncurated_capture_age_hours: null,
  top_tags: [],
  top_files: [],
  memory_type_breakdown: [],
  source_kind_breakdown: [],
  automation: null,
  watchers: null,
};

export default function App() {
  const [tab, setTab] = useState<Tab>("memories");
  const [project, setProject] = useState(localStorage.getItem("memory-layer.project") ?? "memory");
  const [projectInput, setProjectInput] = useState(project);
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);
  const [overview, setOverview] = useState<ProjectOverviewResponse>({ ...EMPTY_OVERVIEW, project });
  const [memories, setMemories] = useState<ProjectMemoriesResponse>({ project, total: 0, items: [] });
  const [selectedMemoryId, setSelectedMemoryId] = useState<string | null>(null);
  const [selectedMemory, setSelectedMemory] = useState<MemoryEntryResponse | null>(null);
  const [queryText, setQueryText] = useState("");
  const [queryResponse, setQueryResponse] = useState<QueryResponse | null>(null);
  const [selectedQueryMemory, setSelectedQueryMemory] = useState<MemoryEntryResponse | null>(null);
  const [selectedQueryIndex, setSelectedQueryIndex] = useState(0);
  const [activities, setActivities] = useState<ActivityEvent[]>([]);
  const [selectedActivityIndex, setSelectedActivityIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState("Connecting to Memory Layer...");
  const [connectionState, setConnectionState] = useState<"connecting" | "live" | "offline">("connecting");
  const [textFilter, setTextFilter] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [typeFilter, setTypeFilter] = useState<MemoryTypeFilter>("all");
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    localStorage.setItem("memory-layer.project", project);
  }, [project]);

  useEffect(() => {
    void refreshProject(project);
    setActivities([]);
    setSelectedActivityIndex(0);
  }, [project]);

  useEffect(() => {
    const socket = new WebSocket(websocketUrl());
    wsRef.current = socket;
    setConnectionState("connecting");

    socket.addEventListener("open", () => {
      setConnectionState("live");
      sendStream({ type: "subscribe_project", project }, socket);
      if (selectedMemoryId) {
        sendStream({ type: "subscribe_memory", memory_id: selectedMemoryId }, socket);
      }
    });

    socket.addEventListener("message", (event) => {
      const payload = JSON.parse(String(event.data)) as StreamResponse;
      if (payload.type === "project_snapshot" || payload.type === "project_changed") {
        setOverview(payload.overview);
        setMemories(payload.memories);
      } else if (payload.type === "memory_snapshot" || payload.type === "memory_changed") {
        setSelectedMemory(payload.detail);
      } else if (payload.type === "activity") {
        setActivities((current) => [payload.event, ...current].slice(0, 200));
      } else if (payload.type === "error") {
        setStatusMessage(payload.message);
      }
    });

    socket.addEventListener("close", () => {
      setConnectionState("offline");
      setStatusMessage("Live connection lost. The page still works, but updates are no longer streaming.");
    });

    socket.addEventListener("error", () => {
      setConnectionState("offline");
    });

    return () => {
      socket.close();
      wsRef.current = null;
    };
  }, [project, selectedMemoryId]);

  useEffect(() => {
    if (!selectedMemoryId) {
      setSelectedMemory(null);
      sendStream({ type: "unsubscribe_memory" });
      return;
    }
    void getMemory(selectedMemoryId)
      .then(setSelectedMemory)
      .catch((error: Error) => setStatusMessage(error.message));
    sendStream({ type: "subscribe_memory", memory_id: selectedMemoryId });
  }, [selectedMemoryId]);

  useEffect(() => {
    const result = queryResponse?.results[selectedQueryIndex];
    if (!result) {
      setSelectedQueryMemory(null);
      return;
    }
    void getMemory(result.memory_id)
      .then(setSelectedQueryMemory)
      .catch((error: Error) => setStatusMessage(error.message));
  }, [queryResponse, selectedQueryIndex]);

  const filteredMemories = useMemo(() => {
    return memories.items.filter((item) => {
      if (textFilter) {
        const haystack = `${item.summary} ${item.preview}`.toLowerCase();
        if (!haystack.includes(textFilter.toLowerCase())) {
          return false;
        }
      }
      if (tagFilter) {
        if (!item.tags.some((tag) => tag.toLowerCase().includes(tagFilter.toLowerCase()))) {
          return false;
        }
      }
      if (statusFilter !== "all" && item.status !== statusFilter) {
        return false;
      }
      if (typeFilter !== "all" && item.memory_type !== typeFilter) {
        return false;
      }
      return true;
    });
  }, [memories.items, statusFilter, tagFilter, textFilter, typeFilter]);

  useEffect(() => {
    if (!filteredMemories.length) {
      setSelectedMemoryId(null);
      return;
    }
    if (!selectedMemoryId || !filteredMemories.some((item) => item.id === selectedMemoryId)) {
      setSelectedMemoryId(filteredMemories[0].id);
    }
  }, [filteredMemories, selectedMemoryId]);

  async function refreshProject(nextProject: string) {
    try {
      const [healthPayload, overviewPayload, memoriesPayload] = await Promise.all([
        getHealth(),
        getOverview(nextProject),
        getMemories(nextProject),
      ]);
      setHealth(healthPayload);
      setOverview(overviewPayload);
      setMemories(memoriesPayload);
      setSelectedMemoryId(memoriesPayload.items[0]?.id ?? null);
      setStatusMessage(`Loaded ${memoriesPayload.items.length} visible memories for ${nextProject}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleQuerySubmit(event: React.FormEvent) {
    event.preventDefault();
    const trimmed = queryText.trim();
    if (!trimmed) {
      setStatusMessage("Enter a query before running search.");
      return;
    }
    try {
      setStatusMessage(`Running query for "${trimmed}"...`);
      const response = await runQuery({
        project,
        query: trimmed,
        filters: {},
        top_k: 8,
        min_confidence: null,
      });
      setQueryResponse(response);
      setSelectedQueryIndex(0);
      setStatusMessage(`Query returned ${response.results.length} memories.`);
      setTab("query");
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function runProjectAction(action: "curate" | "reindex" | "archive") {
    try {
      if (action === "curate") {
        const response = await curate(project);
        setStatusMessage(`Curated ${response.input_count} captures into ${response.output_count} memories.`);
      } else if (action === "reindex") {
        const response = await reindex(project);
        setStatusMessage(`Reindexed ${response.reindexed_entries} memories.`);
      } else {
        const response = await archiveProject(project);
        setStatusMessage(`Archived ${response.archived_count} low-value memories.`);
      }
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleDelete(memoryId: string) {
    try {
      const response = await deleteMemory(memoryId);
      setStatusMessage(`Deleted memory: ${response.summary}`);
      setQueryResponse((current) =>
        current
          ? { ...current, results: current.results.filter((item) => item.memory_id !== memoryId) }
          : current,
      );
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  function applyProjectInput() {
    const next = projectInput.trim();
    if (!next) {
      return;
    }
    setProject(next);
  }

  const activeActivity = activities[selectedActivityIndex] ?? null;
  const activeQueryResult = queryResponse?.results[selectedQueryIndex] ?? null;
  const serviceVersion = typeof health?.version === "string" ? health.version : "unknown";

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">Built for coding agents</p>
          <h1>Memory Layer Web</h1>
        </div>
        <form
          className="project-form"
          onSubmit={(event) => {
            event.preventDefault();
            applyProjectInput();
          }}
        >
          <label>
            Project
            <input value={projectInput} onChange={(event) => setProjectInput(event.target.value)} />
          </label>
          <button type="submit">Load</button>
        </form>
      </header>

      <section className="hero-panel">
        <div className="hero-card">
          <span className={`status-pill status-${connectionState}`}>{connectionState}</span>
          <strong>{overview.project}</strong>
          <span>
            service {overview.service_status} / database {overview.database_status}
          </span>
        </div>
        <div className="hero-card">
          <strong>{overview.memory_entries_total}</strong>
          <span>memories</span>
        </div>
        <div className="hero-card">
          <strong>{overview.raw_captures_total}</strong>
          <span>raw captures</span>
        </div>
        <div className="hero-card">
          <strong>{overview.watchers?.active_count ?? 0}</strong>
          <span>watchers</span>
        </div>
        <div className="hero-card">
          <strong>{serviceVersion}</strong>
          <span>mem-service</span>
        </div>
      </section>

      <nav className="tabs">
        {(["memories", "query", "activity", "project"] as Tab[]).map((name) => (
          <button
            key={name}
            className={tab === name ? "tab-active" : ""}
            onClick={() => setTab(name)}
            type="button"
          >
            {name}
          </button>
        ))}
      </nav>

      {tab === "memories" ? (
        <section className="panel-grid">
          <div className="panel">
            <div className="panel-toolbar filters-grid">
              <input placeholder="Search summary or preview" value={textFilter} onChange={(e) => setTextFilter(e.target.value)} />
              <input placeholder="Filter tag" value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} />
              <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}>
                <option value="all">All statuses</option>
                <option value="active">Active</option>
                <option value="archived">Archived</option>
              </select>
              <select value={typeFilter} onChange={(e) => setTypeFilter(e.target.value as MemoryTypeFilter)}>
                <option value="all">All types</option>
                <option value="architecture">Architecture</option>
                <option value="convention">Convention</option>
                <option value="decision">Decision</option>
                <option value="incident">Incident</option>
                <option value="debugging">Debugging</option>
                <option value="environment">Environment</option>
                <option value="domain_fact">Domain fact</option>
              </select>
            </div>
            <div className="list-view">
              {filteredMemories.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  className={`list-item ${selectedMemoryId === item.id ? "selected" : ""}`}
                  onClick={() => setSelectedMemoryId(item.id)}
                >
                  <div>
                    <strong>{item.summary}</strong>
                    <p>{item.preview}</p>
                  </div>
                  <div className="meta-stack">
                    <span className="badge">{item.memory_type}</span>
                    <span className={`badge badge-${item.status}`}>{item.status}</span>
                    <span>{item.confidence.toFixed(2)}</span>
                  </div>
                </button>
              ))}
            </div>
          </div>
          <div className="panel detail-scroll">
            {selectedMemory ? (
              <>
                <div className="detail-header">
                  <div>
                    <h2>{selectedMemory.summary}</h2>
                    <p>{selectedMemory.memory_type} · {selectedMemory.status} · confidence {selectedMemory.confidence.toFixed(2)}</p>
                  </div>
                  <button className="danger" onClick={() => void handleDelete(selectedMemory.id)} type="button">
                    Delete
                  </button>
                </div>
                <section className="detail-section">
                  <h3>Canonical text</h3>
                  <p>{selectedMemory.canonical_text}</p>
                </section>
                <section className="detail-section">
                  <h3>Tags</h3>
                  <div className="tag-wrap">{selectedMemory.tags.map((tag) => <span key={tag} className="tag">{tag}</span>)}</div>
                </section>
                <section className="detail-section">
                  <h3>Sources</h3>
                  {selectedMemory.sources.map((source) => (
                    <div key={source.id} className="source-card">
                      <strong>{source.source_kind}</strong>
                      <p>{source.file_path ?? source.git_commit ?? "<no path>"}</p>
                      {source.excerpt ? <pre>{source.excerpt}</pre> : null}
                    </div>
                  ))}
                </section>
                <section className="detail-section">
                  <h3>Related memories</h3>
                  {selectedMemory.related_memories.length ? (
                    selectedMemory.related_memories.map((related) => (
                      <div key={`${related.relation_type}-${related.memory_id}`} className="relation-row">
                        <span className="badge">{related.relation_type}</span>
                        <span>{related.summary}</span>
                      </div>
                    ))
                  ) : (
                    <p className="muted">No related memories recorded.</p>
                  )}
                </section>
              </>
            ) : (
              <p className="muted">Select a memory to inspect its details.</p>
            )}
          </div>
        </section>
      ) : null}

      {tab === "query" ? (
        <section className="panel-stack">
          <form className="panel" onSubmit={handleQuerySubmit}>
            <div className="panel-toolbar">
              <input
                className="query-input"
                placeholder="Ask what the project knows..."
                value={queryText}
                onChange={(event) => setQueryText(event.target.value)}
              />
              <button type="submit">Query</button>
            </div>
            {queryResponse ? (
              <div className="query-summary">
                <p>{queryResponse.answer}</p>
                <div className="stats-row">
                  <span>confidence {queryResponse.confidence.toFixed(2)}</span>
                  <span>{queryResponse.insufficient_evidence ? "insufficient evidence" : "sufficient evidence"}</span>
                  <span>lexical {queryResponse.diagnostics.lexical_candidates}</span>
                  <span>semantic {queryResponse.diagnostics.semantic_candidates}</span>
                  <span>merged {queryResponse.diagnostics.merged_candidates}</span>
                  <span>total {queryResponse.diagnostics.total_duration_ms} ms</span>
                </div>
              </div>
            ) : (
              <p className="muted">Run a query to inspect the returned memories and diagnostics.</p>
            )}
          </form>
          <section className="panel-grid">
            <div className="panel">
              <div className="list-view">
                {(queryResponse?.results ?? []).map((result, index) => (
                  <button
                    key={result.memory_id}
                    type="button"
                    className={`list-item ${selectedQueryIndex === index ? "selected" : ""}`}
                    onClick={() => setSelectedQueryIndex(index)}
                  >
                    <div>
                      <strong>{result.summary}</strong>
                      <p>{result.snippet}</p>
                    </div>
                    <div className="meta-stack">
                      <span className="badge">{result.memory_type}</span>
                      <span className="badge">{result.match_kind}</span>
                      <span>{result.score.toFixed(2)}</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
            <div className="panel detail-scroll">
              {activeQueryResult ? (
                <>
                  <div className="detail-header">
                    <div>
                      <h2>{activeQueryResult.summary}</h2>
                      <p>
                        {activeQueryResult.memory_type} · {activeQueryResult.match_kind} · score {activeQueryResult.score.toFixed(2)}
                      </p>
                    </div>
                    <button className="danger" onClick={() => void handleDelete(activeQueryResult.memory_id)} type="button">
                      Delete
                    </button>
                  </div>
                  <section className="detail-section">
                    <h3>Snippet</h3>
                    <p>{activeQueryResult.snippet}</p>
                  </section>
                  <section className="detail-section">
                    <h3>Why it ranked</h3>
                    <ul>
                      {activeQueryResult.score_explanation.map((line) => (
                        <li key={line}>{line}</li>
                      ))}
                    </ul>
                    <div className="stats-row">
                      <span>chunk fts {formatNumber(activeQueryResult.debug.chunk_fts)}</span>
                      <span>entry fts {formatNumber(activeQueryResult.debug.entry_fts)}</span>
                      <span>semantic {formatNumber(activeQueryResult.debug.semantic_similarity)}</span>
                      <span>relation {formatNumber(activeQueryResult.debug.relation_boost)}</span>
                      <span>overlap {Math.round((activeQueryResult.debug.term_overlap ?? 0) * 100)}%</span>
                    </div>
                  </section>
                  {selectedQueryMemory ? (
                    <section className="detail-section">
                      <h3>Memory detail</h3>
                      <p>{selectedQueryMemory.canonical_text}</p>
                    </section>
                  ) : null}
                </>
              ) : (
                <p className="muted">Select a returned memory to inspect its ranking details.</p>
              )}
            </div>
          </section>
        </section>
      ) : null}

      {tab === "activity" ? (
        <section className="panel-grid">
          <div className="panel">
            <div className="list-view">
              {activities.map((event, index) => (
                <button
                  key={`${event.recorded_at}-${event.kind}-${index}`}
                  type="button"
                  className={`list-item ${selectedActivityIndex === index ? "selected" : ""}`}
                  onClick={() => setSelectedActivityIndex(index)}
                >
                  <div>
                    <strong>{event.kind}</strong>
                    <p>{event.summary}</p>
                  </div>
                  <span>{formatDateTime(event.recorded_at)}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="panel detail-scroll">
            {activeActivity ? (
              <>
                <h2>{activeActivity.kind}</h2>
                <p>{activeActivity.summary}</p>
                <p className="muted">{formatDateTime(activeActivity.recorded_at)}</p>
                <ActivityDetail details={activeActivity.details} />
              </>
            ) : (
              <p className="muted">Keep this page open while queries, captures, curation runs, and deletions happen.</p>
            )}
          </div>
        </section>
      ) : null}

      {tab === "project" ? (
        <section className="panel-stack">
          <div className="panel actions-row">
            <button onClick={() => void refreshProject(project)} type="button">Refresh</button>
            <button onClick={() => void runProjectAction("curate")} type="button">Curate</button>
            <button onClick={() => void runProjectAction("reindex")} type="button">Reindex</button>
            <button onClick={() => void runProjectAction("archive")} type="button">Archive</button>
          </div>
          <section className="project-grid">
            <div className="panel">
              <h2>Overview</h2>
              <Metric label="Service" value={`${overview.service_status} / ${overview.database_status}`} />
              <Metric label="Memories" value={`${overview.memory_entries_total} total / ${overview.active_memories} active / ${overview.archived_memories} archived`} />
              <Metric label="Confidence bins" value={`${overview.high_confidence_memories} high / ${overview.medium_confidence_memories} medium / ${overview.low_confidence_memories} low`} />
              <Metric label="Recent 7d" value={`${overview.recent_memories_7d} memories / ${overview.recent_captures_7d} captures`} />
              <Metric label="Raw captures" value={`${overview.raw_captures_total} total / ${overview.uncurated_raw_captures} uncurated`} />
              <Metric label="Tasks / Sessions / Runs" value={`${overview.tasks_total} / ${overview.sessions_total} / ${overview.curation_runs_total}`} />
              <Metric label="Last memory" value={formatDateTime(overview.last_memory_at)} />
              <Metric label="Last curation" value={formatDateTime(overview.last_curation_at)} />
              <Metric label="Last capture" value={formatDateTime(overview.last_capture_at)} />
              <Metric
                label="Automation"
                value={
                  overview.automation
                    ? `${overview.automation.mode} · dirty ${overview.automation.dirty_file_count} · pending ${overview.automation.pending_capture_count}`
                    : "not configured"
                }
              />
            </div>
            <div className="panel">
              <h2>Watchers</h2>
              {overview.watchers?.watchers.length ? (
                overview.watchers.watchers.map((watcher) => (
                  <div key={watcher.watcher_id} className="watcher-card">
                    <strong>{watcher.hostname}</strong>
                    <p>{watcher.repo_root}</p>
                    <div className="stats-row">
                      <span>pid {watcher.pid}</span>
                      <span>{watcher.mode}</span>
                      <span>{formatDateTime(watcher.last_heartbeat_at)}</span>
                    </div>
                  </div>
                ))
              ) : (
                <p className="muted">No watcher presence reported.</p>
              )}
            </div>
            <div className="panel">
              <h2>Top tags</h2>
              <KeyValueList items={overview.top_tags.map((item) => [item.name, String(item.count)])} empty="No tags yet." />
            </div>
            <div className="panel">
              <h2>Top files</h2>
              <KeyValueList items={overview.top_files.map((item) => [item.name, String(item.count)])} empty="No file provenance yet." />
            </div>
          </section>
        </section>
      ) : null}

      <footer className="statusbar">{statusMessage}</footer>
    </div>
  );

  function sendStream(request: StreamRequest, socket = wsRef.current) {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return;
    }
    socket.send(JSON.stringify(request));
  }
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function KeyValueList({ items, empty }: { items: [string, string][]; empty: string }) {
  if (!items.length) {
    return <p className="muted">{empty}</p>;
  }
  return (
    <div className="kv-list">
      {items.map(([key, value]) => (
        <div className="kv-row" key={key}>
          <span>{key}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </div>
  );
}

function ActivityDetail({ details }: { details: ActivityDetails | null }) {
  if (!details) {
    return <p className="muted">No structured details recorded.</p>;
  }

  return (
    <div className="detail-section">
      <h3>Details</h3>
      <pre>{JSON.stringify(details, null, 2)}</pre>
    </div>
  );
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) {
    return "n/a";
  }
  return new Date(value).toLocaleString();
}

function formatNumber(value: number | null | undefined): string {
  return typeof value === "number" ? value.toFixed(2) : "0.00";
}

function websocketUrl(): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws`;
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  activateEmbeddingBackend,
  approveProposal,
  archiveProject,
  curate,
  deactivateEmbeddingBackend,
  deleteMemory,
  exportBundle,
  getActivities,
  getAgentSnapshot,
  getEmbeddingBackends,
  getHealth,
  getLlmAuditStatus,
  getMemory,
  getMemoryHistory,
  getMemories,
  getOverview,
  getReplacementPolicy,
  getReplacementProposals,
  getRuntimeStatus,
  getResume,
  getUpToSpeed,
  importBundle,
  previewExportBundle,
  previewImportBundle,
  reembed,
  reindex,
  rejectProposal,
  runQuery,
  saveReplacementPolicy,
  setEmbeddingCreationEnabled,
  setLlmAuditEnabled,
} from "../api";
import { collectErrorItems } from "../features/errors/errorItems";
import type { MemoryTypeFilter, StatusFilter } from "../features/memories/MemoriesTab";
import { PRIMARY_TABS, type Tab } from "../tabs";
import type {
  ActivityEvent,
  AgentSnapshotResponse,
  DiagnosticInfo,
  EmbeddingBackendInfo,
  EmbeddingBackendsResponse,
  LlmAuditStatusResponse,
  MemoryEntryResponse,
  MemoryHistoryResponse,
  ProjectMemoryBundlePreview,
  ProjectMemoryExportOptions,
  ProjectMemoryImportPreview,
  ProjectMemoriesResponse,
  ProjectOverviewResponse,
  QueryResponse,
  ReplacementPolicy,
  ReplacementPolicyResponse,
  ReplacementProposalRecord,
  ResumeResponse,
  RuntimeStatusResponse,
  StreamRequest,
  StreamResponse,
  UpToSpeedResponse,
} from "../types";
import { mergeActivityEventLists, mergeActivityEvents } from "../utils/activity";
import { websocketUrl } from "../utils/network";

interface QueryHistoryEntry {
  question: string;
  response: QueryResponse;
  roundtripMs: number;
}

function embeddingBackendSelectionIndex(
  payload: EmbeddingBackendsResponse,
  preferredName: string | null,
  fallbackIndex: number,
): number {
  if (!payload.backends.length) return 0;
  if (preferredName) {
    const preferredIndex = payload.backends.findIndex((backend) => backend.name === preferredName);
    if (preferredIndex >= 0) return preferredIndex;
  }
  const activeIndex = payload.backends.findIndex((backend) => backend.active);
  if (activeIndex >= 0) return activeIndex;
  return Math.min(fallbackIndex, payload.backends.length - 1);
}

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
  embedding_chunks_total: 0,
  fresh_embedding_chunks: 0,
  stale_embedding_chunks: 0,
  missing_embedding_chunks: 0,
  embedding_spaces_total: 0,
  active_embedding_provider: null,
  active_embedding_model: null,
  pending_replacement_proposals: 0,
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

export function useWebAppController() {
  const [tab, setTab] = useState<Tab>("memories");
  const [project, setProject] = useState(localStorage.getItem("memory-layer.project") ?? "memory");
  const [projectInput, setProjectInput] = useState(project);
  const [repoRootInput, setRepoRootInput] = useState(localStorage.getItem("memory-layer.repoRoot") ?? "");
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);
  const [overview, setOverview] = useState<ProjectOverviewResponse>({ ...EMPTY_OVERVIEW, project });
  const [memories, setMemories] = useState<ProjectMemoriesResponse>({ project, total: 0, items: [] });
  const [selectedMemoryId, setSelectedMemoryId] = useState<string | null>(null);
  const [selectedMemory, setSelectedMemory] = useState<MemoryEntryResponse | null>(null);
  const [selectedHistory, setSelectedHistory] = useState<MemoryHistoryResponse | null>(null);
  const [queryText, setQueryText] = useState("");
  const [queryResponse, setQueryResponse] = useState<QueryResponse | null>(null);
  const [selectedQueryMemory, setSelectedQueryMemory] = useState<MemoryEntryResponse | null>(null);
  const [selectedQueryIndex, setSelectedQueryIndex] = useState(0);
  const [queryLoading, setQueryLoading] = useState(false);
  const [queryError, setQueryError] = useState<string | null>(null);
  const [queryRoundtripMs, setQueryRoundtripMs] = useState<number | null>(null);
  const [queryHistory, setQueryHistory] = useState<QueryHistoryEntry[]>([]);
  const [queryHistoryCursor, setQueryHistoryCursor] = useState<number | null>(null);
  const [selectedQueryMemoryLoading, setSelectedQueryMemoryLoading] = useState(false);
  const [selectedQueryMemoryError, setSelectedQueryMemoryError] = useState<string | null>(null);
  const [activities, setActivities] = useState<ActivityEvent[]>([]);
  const [selectedActivityIndex, setSelectedActivityIndex] = useState(0);
  const [localDiagnostics, setLocalDiagnostics] = useState<DiagnosticInfo[]>([]);
  const [selectedErrorIndex, setSelectedErrorIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState("Connecting to Memory Layer...");
  const [connectionState, setConnectionState] = useState<"connecting" | "live" | "offline">("connecting");
  const [runtimeStatus, setRuntimeStatus] = useState<RuntimeStatusResponse | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const [textFilter, setTextFilter] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [typeFilter, setTypeFilter] = useState<MemoryTypeFilter>("all");
  const [bundleOptions, setBundleOptions] = useState<ProjectMemoryExportOptions>({
    include_archived: false,
    include_tags: true,
    include_relations: true,
    include_source_file_paths: false,
    include_git_commits: false,
    include_source_excerpts: false,
  });
  const [exportPreview, setExportPreview] = useState<ProjectMemoryBundlePreview | null>(null);
  const [importPreview, setImportPreview] = useState<ProjectMemoryImportPreview | null>(null);
  const [importFile, setImportFile] = useState<File | null>(null);
  // Agents state
  const [agentSnapshot, setAgentSnapshot] = useState<AgentSnapshotResponse | null>(null);
  const [selectedAgentIndex, setSelectedAgentIndex] = useState(0);
  // Embeddings state
  const [embeddingBackends, setEmbeddingBackends] = useState<EmbeddingBackendsResponse | null>(null);
  const [selectedEmbeddingIndex, setSelectedEmbeddingIndex] = useState(0);
  const [embeddingLoading, setEmbeddingLoading] = useState(false);
  const [embeddingOperation, setEmbeddingOperation] = useState<string | null>(null);
  // Resume state
  const [resumeData, setResumeData] = useState<ResumeResponse | null>(null);
  const [resumeLoading, setResumeLoading] = useState(false);
  const [resumeAutoloadedFor, setResumeAutoloadedFor] = useState<string | null>(null);
  // Activity briefing state
  const [upToSpeed, setUpToSpeed] = useState<UpToSpeedResponse | null>(null);
  const [upToSpeedLoading, setUpToSpeedLoading] = useState(false);
  const [upToSpeedError, setUpToSpeedError] = useState<string | null>(null);
  const [llmAudit, setLlmAudit] = useState<LlmAuditStatusResponse | null>(null);
  const [llmAuditLoading, setLlmAuditLoading] = useState(false);
  const [llmAuditError, setLlmAuditError] = useState<string | null>(null);
  const [llmAuditToggling, setLlmAuditToggling] = useState(false);
  // Proposals state
  const [proposals, setProposals] = useState<ReplacementProposalRecord[]>([]);
  const [selectedProposalIndex, setSelectedProposalIndex] = useState(0);
  const [replacementPolicy, setReplacementPolicy] = useState<ReplacementPolicyResponse | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const queryRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    localStorage.setItem("memory-layer.project", project);
  }, [project]);

  useEffect(() => {
    localStorage.setItem("memory-layer.repoRoot", repoRootInput);
  }, [repoRootInput]);

  useEffect(() => {
    void refreshProject(project);
    setActivities([]);
    setSelectedActivityIndex(0);
    void getActivities(project, 100)
      .then((response) =>
        setActivities((current) => mergeActivityEventLists(response.items, current).slice(0, 200)),
      )
      .catch((error: Error) => {
        setStatusMessage(error.message);
        recordLocalDiagnostic("activity", "load", error.message);
      });
  }, [project]);

  // WebSocket
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
        setActivities((current) => mergeActivityEvents(payload.event, current).slice(0, 200));
      } else if (payload.type === "error") {
        setStatusMessage(payload.message);
        recordLocalDiagnostic("websocket", "stream", payload.message);
      }
    });

    socket.addEventListener("close", () => {
      setConnectionState("offline");
      setStatusMessage("Live connection lost. The page still works, but updates are no longer streaming.");
      recordLocalDiagnostic("websocket", "close", "Live connection lost.");
    });

    socket.addEventListener("error", () => {
      setConnectionState("offline");
      recordLocalDiagnostic("websocket", "error", "WebSocket connection failed.");
    });

    return () => {
      socket.close();
      wsRef.current = null;
    };
  }, [project, selectedMemoryId]);

  // Memory detail subscription
  useEffect(() => {
    if (!selectedMemoryId) {
      setSelectedMemory(null);
      setSelectedHistory(null);
      sendStream({ type: "unsubscribe_memory" });
      return;
    }
    setSelectedHistory(null);
    void getMemory(selectedMemoryId)
      .then(setSelectedMemory)
      .catch((error: Error) => setStatusMessage(error.message));
    sendStream({ type: "subscribe_memory", memory_id: selectedMemoryId });
  }, [selectedMemoryId]);

  // Query result detail
  useEffect(() => {
    const result = queryResponse?.results[selectedQueryIndex];
    if (!result) {
      setSelectedQueryMemory(null);
      setSelectedQueryMemoryLoading(false);
      setSelectedQueryMemoryError(null);
      return;
    }
    let active = true;
    setSelectedQueryMemory(null);
    setSelectedQueryMemoryLoading(true);
    setSelectedQueryMemoryError(null);
    void getMemory(result.memory_id)
      .then((detail) => {
        if (active) setSelectedQueryMemory(detail);
      })
      .catch((error: Error) => {
        if (active) {
          setSelectedQueryMemoryError(error.message);
          setStatusMessage(error.message);
        }
      })
      .finally(() => {
        if (active) setSelectedQueryMemoryLoading(false);
      });
    return () => {
      active = false;
    };
  }, [queryResponse, selectedQueryIndex]);

  // Agent polling (every 2s when on agents tab)
  useEffect(() => {
    if (tab !== "agents") return;
    let active = true;
    const poll = () => {
      void getAgentSnapshot()
        .then((snap) => { if (active) setAgentSnapshot(snap); })
        .catch(() => {});
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => { active = false; clearInterval(id); };
  }, [tab]);

  // Load proposals and replacement policy when review tab is active
  useEffect(() => {
    if (tab !== "review") return;
    void refreshReview();
  }, [tab, project]);

  // Load embeddings when embeddings tab is active
  useEffect(() => {
    if (tab !== "embeddings") return;
    void refreshEmbeddings();
  }, [tab, project]);

  useEffect(() => {
    if (tab !== "activity") return;
    void refreshLlmAuditStatus();
  }, [tab]);

  const filteredMemories = useMemo(() => {
    return memories.items.filter((item) => {
      if (textFilter) {
        const haystack = `${item.summary} ${item.preview}`.toLowerCase();
        if (!haystack.includes(textFilter.toLowerCase())) return false;
      }
      if (tagFilter) {
        if (!item.tags.some((t) => t.toLowerCase().includes(tagFilter.toLowerCase()))) return false;
      }
      if (statusFilter !== "all" && item.status !== statusFilter) return false;
      if (typeFilter !== "all" && item.memory_type !== typeFilter) return false;
      return true;
    });
  }, [memories.items, statusFilter, tagFilter, textFilter, typeFilter]);

  const effectiveRepoRoot = useMemo(() => {
    const manual = repoRootInput.trim();
    if (manual) return manual;
    const automationRoot = overview.automation?.repo_root?.trim();
    if (automationRoot) return automationRoot;
    const roots = Array.from(new Set((overview.watchers?.watchers ?? []).map((watcher) => watcher.repo_root).filter(Boolean)));
    return roots.length === 1 ? roots[0] : "";
  }, [overview.automation?.repo_root, overview.watchers?.watchers, repoRootInput]);

  const sortedAgentSessions = useMemo(() => {
    const sessions = [...(agentSnapshot?.sessions ?? [])];
    sessions.sort((left, right) => {
      const leftCurrent = left.project_name === project || left.cwd === effectiveRepoRoot;
      const rightCurrent = right.project_name === project || right.cwd === effectiveRepoRoot;
      if (leftCurrent !== rightCurrent) return leftCurrent ? -1 : 1;
      return right.started_at - left.started_at;
    });
    return sessions;
  }, [agentSnapshot?.sessions, effectiveRepoRoot, project]);

  const errorItems = collectErrorItems(activities, localDiagnostics, connectionState);

  const selectedEmbeddingBackend = embeddingBackends?.backends[selectedEmbeddingIndex] ?? null;
  const embeddingBusy = embeddingLoading || embeddingOperation !== null;

  useEffect(() => {
    let active = true;
    const refreshRuntimeStatus = () => {
      void getRuntimeStatus(project, effectiveRepoRoot || null)
        .then((payload) => {
          if (active) setRuntimeStatus(payload);
        })
        .catch((error: Error) => {
          if (active) recordLocalDiagnostic("runtime", "status", error.message);
        });
    };
    refreshRuntimeStatus();
    const id = setInterval(refreshRuntimeStatus, 5000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, [effectiveRepoRoot, project]);

  useEffect(() => {
    if (!effectiveRepoRoot) return;
    const key = `${project}:${effectiveRepoRoot}`;
    if (resumeAutoloadedFor === key) return;
    setResumeAutoloadedFor(key);
    void getResume(project, effectiveRepoRoot, false)
      .then((data) => {
        setResumeData(data);
        if (data.checkpoint && (data.timeline.length || data.commits.length || data.changed_memories.length)) {
          setTab("resume");
        }
      })
      .catch(() => {});
  }, [effectiveRepoRoot, project, resumeAutoloadedFor]);

  useEffect(() => {
    if (!filteredMemories.length) {
      setSelectedMemoryId(null);
      return;
    }
    if (!selectedMemoryId || !filteredMemories.some((item) => item.id === selectedMemoryId)) {
      setSelectedMemoryId(filteredMemories[0].id);
    }
  }, [filteredMemories, selectedMemoryId]);

  useEffect(() => {
    setSelectedAgentIndex((current) => Math.min(current, Math.max(sortedAgentSessions.length - 1, 0)));
  }, [sortedAgentSessions.length]);

  useEffect(() => {
    setSelectedErrorIndex((current) => Math.min(current, Math.max(errorItems.length - 1, 0)));
  }, [errorItems.length]);

  // Keyboard shortcuts
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      const inInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";

      if (!inInput && (e.key === "h" || e.key === "H")) {
        e.preventDefault();
        setHelpOpen((current) => !current);
        return;
      }
      if (!inInput && e.key === "Escape" && helpOpen) {
        e.preventDefault();
        setHelpOpen(false);
        return;
      }

      if (inInput) return;

      const tabIndex = parseInt(e.key, 10) - 1;
      if (tabIndex >= 0 && tabIndex < PRIMARY_TABS.length) {
        e.preventDefault();
        setTab(PRIMARY_TABS[tabIndex]);
        return;
      }

      if (e.key === "/" && tab === "memories") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (e.key === "?" && tab === "query") {
        e.preventDefault();
        queryRef.current?.focus();
        return;
      }
      if (tab === "embeddings") {
        if (e.key === "r") {
          e.preventDefault();
          void refreshEmbeddings();
          return;
        }
        if (e.key === "Enter" && selectedEmbeddingBackend && !embeddingBusy) {
          e.preventDefault();
          void handleToggleEmbeddingSearch(selectedEmbeddingBackend);
          return;
        }
        if (e.key === "c" && selectedEmbeddingBackend && !embeddingBusy) {
          e.preventDefault();
          void handleToggleEmbeddingCreation(selectedEmbeddingBackend);
          return;
        }
        if (e.key === "e" && selectedEmbeddingBackend && !embeddingBusy) {
          e.preventDefault();
          void handleReembedEmbeddingBackend(selectedEmbeddingBackend);
          return;
        }
        if (e.key === "I" && selectedEmbeddingBackend && !embeddingBusy) {
          e.preventDefault();
          void handleReindexEmbeddingBackend(selectedEmbeddingBackend);
          return;
        }
      }
      if (e.key === "r") {
        e.preventDefault();
        void refreshProject(project);
        return;
      }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [embeddingBusy, helpOpen, project, selectedEmbeddingBackend, tab]);

  const refreshProject = useCallback(async (nextProject: string) => {
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
  }, []);

  async function handleQuerySubmit(event: React.FormEvent) {
    event.preventDefault();
    const trimmed = queryText.trim();
    if (!trimmed) {
      setQueryResponse(null);
      setQueryError(null);
      setQueryRoundtripMs(null);
      setStatusMessage("Enter a query before running search.");
      return;
    }
    const started = performance.now();
    setQueryLoading(true);
    setQueryError(null);
    setQueryHistoryCursor(null);
    try {
      setStatusMessage(`Running query for "${trimmed}"...`);
      const response = await runQuery({
        project,
        query: trimmed,
        filters: {},
        top_k: 8,
        min_confidence: null,
        history: false,
      });
      setQueryResponse(response);
      setSelectedQueryIndex(0);
      const roundtripMs = Math.round(performance.now() - started);
      setQueryRoundtripMs(roundtripMs);
      setQueryHistory((current) => {
        const entry = { question: trimmed, response, roundtripMs };
        const withoutDuplicateTail = current[current.length - 1]?.question === trimmed ? current.slice(0, -1) : current;
        return [...withoutDuplicateTail, entry].slice(-50);
      });
      setStatusMessage(`Query returned ${response.results.length} memories in ${roundtripMs} ms.`);
      setTab("query");
    } catch (error) {
      const message = (error as Error).message;
      setQueryError(message);
      setQueryRoundtripMs(Math.round(performance.now() - started));
      setStatusMessage(message);
      recordLocalDiagnostic("query", "run", message);
    } finally {
      setQueryLoading(false);
    }
  }

  function applyQueryHistory(delta: number) {
    if (!queryHistory.length) {
      setStatusMessage("No previous queries in this browser session.");
      return;
    }
    const last = queryHistory.length - 1;
    let next: number | null = queryHistoryCursor;
    if (queryHistoryCursor === null && delta < 0) next = last;
    else if (queryHistoryCursor === null && delta > 0) next = null;
    else if (queryHistoryCursor !== null && delta < 0) next = Math.max(0, queryHistoryCursor - 1);
    else if (queryHistoryCursor !== null && delta > 0 && queryHistoryCursor >= last) next = null;
    else if (queryHistoryCursor !== null && delta > 0) next = queryHistoryCursor + 1;

    setQueryHistoryCursor(next);
    if (next === null) {
      setQueryText("");
      setStatusMessage("Returned to a new empty query.");
    } else {
      const entry = queryHistory[next];
      setQueryText(entry.question);
      setQueryResponse(entry.response);
      setQueryRoundtripMs(entry.roundtripMs);
      setSelectedQueryIndex(0);
      setQueryError(null);
      setStatusMessage(`Loaded query history item ${next + 1}/${queryHistory.length}.`);
    }
  }

  function recordLocalDiagnostic(component: string, operation: string, message: string) {
    const diagnostic: DiagnosticInfo = {
      code: `${component}_${operation}_failed`,
      source: "web",
      component,
      operation,
      severity: "error",
      message,
      raw_error: message,
      explanation: "This error was observed by the browser UI during the current session.",
      fix_hint: "Refresh the tab or run memory doctor if the problem persists.",
      doctor_hint: "memory doctor",
      command_hint: "memory doctor",
    };
    setLocalDiagnostics((current) => {
      if (current[0]?.code === diagnostic.code && current[0]?.message === diagnostic.message) {
        return current;
      }
      return [diagnostic, ...current].slice(0, 100);
    });
  }

  async function runProjectAction(action: "curate" | "reindex" | "reembed" | "archive") {
    try {
      if (action === "curate") {
        const response = await curate(project);
        setStatusMessage(`Curated ${response.input_count} captures into ${response.output_count} memories with ${response.proposal_count} proposal(s).`);
      } else if (action === "reindex") {
        const response = await reindex(project);
        setStatusMessage(`Reindexed ${response.reindexed_entries} memories.`);
      } else if (action === "reembed") {
        const response = await reembed(project);
        setStatusMessage(`Materialized ${response.reembedded_chunks} chunk embeddings across configured spaces.`);
      } else {
        const response = await archiveProject(project);
        setStatusMessage(`Archived ${response.archived_count} low-value memories.`);
      }
      await refreshProject(project);
      if (tab === "embeddings") await refreshEmbeddings(null, true);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handlePreviewExport() {
    try {
      const preview = await previewExportBundle(project, bundleOptions);
      setExportPreview(preview);
      setStatusMessage(`Prepared export preview for ${preview.memory_count} memories.`);
      setTab("bundles");
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleDownloadExport() {
    try {
      const blob = await exportBundle(project, bundleOptions);
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${project}-memory-bundle.zip`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
      setStatusMessage(`Downloaded export bundle for ${project}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handlePreviewImport() {
    if (!importFile) {
      setStatusMessage("Choose a bundle file first.");
      return;
    }
    try {
      const preview = await previewImportBundle(project, importFile);
      setImportPreview(preview);
      setStatusMessage(`Previewed bundle from ${preview.source_project}.`);
      setTab("bundles");
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleApplyImport() {
    if (!importFile) {
      setStatusMessage("Choose a bundle file first.");
      return;
    }
    try {
      const response = await importBundle(project, importFile);
      setImportPreview(null);
      setStatusMessage(`Imported ${response.imported_count} memories into ${response.target_project}.`);
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

  async function handleLoadResume() {
    setResumeLoading(true);
    try {
      const data = await getResume(project, effectiveRepoRoot || null);
      setResumeData(data);
      setStatusMessage("Resume briefing loaded.");
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setResumeLoading(false);
    }
  }

  async function handleUpToSpeed(includeLlmSummary: boolean) {
    setUpToSpeedLoading(true);
    setUpToSpeedError(null);
    try {
      const data = await getUpToSpeed({ project, include_llm_summary: includeLlmSummary, limit: 20 });
      setUpToSpeed(data);
      setStatusMessage(includeLlmSummary ? "LLM get-up-to-speed briefing loaded." : "Deterministic get-up-to-speed briefing loaded.");
    } catch (error) {
      const message = (error as Error).message;
      setUpToSpeedError(message);
      setStatusMessage(message);
      recordLocalDiagnostic("activity", "up_to_speed", message);
    } finally {
      setUpToSpeedLoading(false);
    }
  }

  async function refreshLlmAuditStatus() {
    setLlmAuditLoading(true);
    setLlmAuditError(null);
    try {
      const status = await getLlmAuditStatus();
      setLlmAudit(status);
    } catch (error) {
      const message = (error as Error).message;
      setLlmAuditError(message);
      recordLocalDiagnostic("activity", "llm_audit_status", message);
    } finally {
      setLlmAuditLoading(false);
    }
  }

  async function handleToggleLlmAudit() {
    const enabled = !(llmAudit?.enabled ?? false);
    setLlmAuditToggling(true);
    setLlmAuditError(null);
    try {
      const status = await setLlmAuditEnabled(enabled);
      setLlmAudit(status);
      setStatusMessage(`LLM audit/debug logging ${status.enabled ? "enabled" : "disabled"}.`);
    } catch (error) {
      const message = (error as Error).message;
      setLlmAuditError(message);
      setStatusMessage(message);
      recordLocalDiagnostic("activity", "llm_audit_toggle", message);
    } finally {
      setLlmAuditToggling(false);
    }
  }

  async function handleLoadHistory(memoryId: string) {
    try {
      if (selectedHistory) {
        setSelectedHistory(null);
        setStatusMessage("Hid version history.");
        return;
      }
      const history = await getMemoryHistory(memoryId);
      setSelectedHistory(history);
      setStatusMessage(`Loaded ${history.versions.length} versions for ${history.canonical_id}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function refreshReview() {
    try {
      const [proposalPayload, policyPayload] = await Promise.all([
        getReplacementProposals(project),
        getReplacementPolicy(project, effectiveRepoRoot || null),
      ]);
      setProposals(proposalPayload.proposals);
      setSelectedProposalIndex((current) => Math.min(current, Math.max(proposalPayload.proposals.length - 1, 0)));
      setReplacementPolicy(policyPayload);
      if (!repoRootInput.trim() && policyPayload.repo_root) {
        setRepoRootInput(policyPayload.repo_root);
      }
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleCyclePolicy() {
    const repoRoot = replacementPolicy?.repo_root || effectiveRepoRoot;
    if (!repoRoot) {
      setStatusMessage("Set a repo root before changing the curation replacement policy.");
      return;
    }
    const current = replacementPolicy?.replacement_policy ?? "balanced";
    const next: ReplacementPolicy =
      current === "conservative" ? "balanced" : current === "balanced" ? "aggressive" : "conservative";
    try {
      const saved = await saveReplacementPolicy(project, {
        repo_root: repoRoot,
        replacement_policy: next,
      });
      setReplacementPolicy(saved);
      setRepoRootInput(saved.repo_root ?? repoRoot);
      setStatusMessage(`Curation replacement policy set to ${saved.replacement_policy}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function refreshEmbeddings(preferredName?: string | null, quiet = false) {
    setEmbeddingLoading(true);
    try {
      const payload = await getEmbeddingBackends(project);
      const currentName = embeddingBackends?.backends[selectedEmbeddingIndex]?.name ?? null;
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) =>
        embeddingBackendSelectionIndex(payload, preferredName ?? currentName, current),
      );
      if (!quiet) setStatusMessage(`Loaded ${payload.backends.length} embedding backend(s).`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingLoading(false);
    }
  }

  async function handleToggleEmbeddingSearch(backend: EmbeddingBackendInfo) {
    if (!backend.ready) {
      setStatusMessage(`Embedding backend ${backend.name} is not ready.`);
      return;
    }
    setEmbeddingOperation(backend.active ? `turning off ${backend.name}` : `activating ${backend.name}`);
    try {
      const payload = backend.active
        ? await deactivateEmbeddingBackend()
        : await activateEmbeddingBackend(backend.name);
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) => embeddingBackendSelectionIndex(payload, backend.name, current));
      setStatusMessage(backend.active ? "Embeddings off." : `Activated embedding backend ${backend.name}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleToggleEmbeddingCreation(backend: EmbeddingBackendInfo) {
    const enabled = !backend.create_enabled;
    setEmbeddingOperation(`toggling automatic creation for ${backend.name}`);
    try {
      const payload = await setEmbeddingCreationEnabled(backend.name, enabled);
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) => embeddingBackendSelectionIndex(payload, backend.name, current));
      setStatusMessage(`Automatic embedding creation ${enabled ? "on" : "off"} for ${backend.name}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleReembedEmbeddingBackend(backend: EmbeddingBackendInfo) {
    setEmbeddingOperation(`creating embeddings for ${backend.name}`);
    try {
      const response = await reembed(project, backend.name);
      setStatusMessage(`Created ${response.reembedded_chunks} chunk embedding(s) for ${backend.name}.`);
      await refreshEmbeddings(backend.name, true);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleReindexEmbeddingBackend(backend: EmbeddingBackendInfo) {
    setEmbeddingOperation(`reindexing ${backend.name}`);
    try {
      const response = await reindex(project, backend.name);
      setStatusMessage(`Reindexed ${response.reindexed_entries} memory entries for ${backend.name}.`);
      await refreshEmbeddings(backend.name, true);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleApproveProposal(proposalId: string) {
    try {
      const res = await approveProposal(project, proposalId);
      setStatusMessage(`Approved: ${res.candidate_summary} replaced ${res.target_summary}`);
      setProposals((prev) => prev.filter((p) => p.id !== proposalId));
      setSelectedProposalIndex((current) => Math.max(0, current - 1));
      await refreshProject(project);
      await refreshReview();
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleRejectProposal(proposalId: string) {
    try {
      const res = await rejectProposal(project, proposalId);
      setStatusMessage(`Rejected proposal for ${res.target_summary}`);
      setProposals((prev) => prev.filter((p) => p.id !== proposalId));
      setSelectedProposalIndex((current) => Math.max(0, current - 1));
      await refreshReview();
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  function applyProjectInput() {
    const next = projectInput.trim();
    if (!next) return;
    setProject(next);
  }

  const activeActivity = activities[selectedActivityIndex] ?? null;
  const activeQueryResult = queryResponse?.results[selectedQueryIndex] ?? null;
  const serviceVersion = typeof health?.version === "string" ? health.version : "unknown";
  const selectedAgent = sortedAgentSessions[selectedAgentIndex] ?? null;
  const activeProposal = proposals[selectedProposalIndex] ?? null;
  const activeError = errorItems[selectedErrorIndex] ?? null;


  function sendStream(request: StreamRequest, socket = wsRef.current) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(request));
  }

  return {
    tab,
    setTab,
    project,
    projectInput,
    setProjectInput,
    repoRootInput,
    setRepoRootInput,
    connectionState,
    overview,
    runtimeStatus,
    serviceVersion,
    helpOpen,
    setHelpOpen,
    applyProjectInput,
    searchRef,
    filteredMemories,
    selectedMemoryId,
    selectedMemory,
    selectedHistory,
    textFilter,
    setTextFilter,
    tagFilter,
    setTagFilter,
    statusFilter,
    setStatusFilter,
    typeFilter,
    setTypeFilter,
    setSelectedMemoryId,
    setSelectedHistory,
    handleLoadHistory,
    handleDelete,
    agentSnapshot,
    sortedAgentSessions,
    selectedAgent,
    selectedAgentIndex,
    setSelectedAgentIndex,
    queryRef,
    queryText,
    setQueryText,
    queryResponse,
    activeQueryResult,
    selectedQueryMemory,
    selectedQueryIndex,
    selectedQueryMemoryLoading,
    selectedQueryMemoryError,
    queryLoading,
    queryError,
    queryRoundtripMs,
    handleQuerySubmit,
    applyQueryHistory,
    setQueryHistoryCursor,
    setSelectedQueryIndex,
    activities,
    activeActivity,
    selectedActivityIndex,
    setSelectedActivityIndex,
    upToSpeed,
    upToSpeedLoading,
    upToSpeedError,
    llmAudit,
    llmAuditLoading,
    llmAuditError,
    llmAuditToggling,
    handleUpToSpeed,
    handleToggleLlmAudit,
    errorItems,
    activeError,
    selectedErrorIndex,
    setSelectedErrorIndex,
    proposals,
    replacementPolicy,
    refreshProject,
    runProjectAction,
    handleApproveProposal,
    handleRejectProposal,
    effectiveRepoRoot,
    activeProposal,
    selectedProposalIndex,
    setSelectedProposalIndex,
    refreshReview,
    handleCyclePolicy,
    embeddingBackends,
    selectedEmbeddingBackend,
    selectedEmbeddingIndex,
    setSelectedEmbeddingIndex,
    embeddingBusy,
    embeddingLoading,
    embeddingOperation,
    refreshEmbeddings,
    handleToggleEmbeddingSearch,
    handleToggleEmbeddingCreation,
    handleReembedEmbeddingBackend,
    handleReindexEmbeddingBackend,
    resumeData,
    resumeLoading,
    handleLoadResume,
    bundleOptions,
    setBundleOptions,
    exportPreview,
    importPreview,
    setImportFile,
    handlePreviewExport,
    handleDownloadExport,
    handlePreviewImport,
    handleApplyImport,
    statusMessage,
  };
}

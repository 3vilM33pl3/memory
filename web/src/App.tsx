import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
  getMemory,
  getMemoryHistory,
  getMemories,
  getOverview,
  getReplacementPolicy,
  getReplacementProposals,
  getResume,
  importBundle,
  previewExportBundle,
  previewImportBundle,
  reembed,
  reindex,
  rejectProposal,
  runQuery,
  saveReplacementPolicy,
  setEmbeddingCreationEnabled,
} from "./api";
import type {
  ActivityEvent,
  AgentSnapshotResponse,
  EmbeddingBackendInfo,
  EmbeddingBackendsResponse,
  MemoryEntryResponse,
  MemoryHistoryResponse,
  MemoryStatus,
  MemoryType,
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
  StreamRequest,
  StreamResponse,
} from "./types";

const PRIMARY_TABS = ["memories", "agents", "query", "project", "review", "watchers", "embeddings", "resume"] as const;
const MORE_TABS = ["bundles", "activity"] as const;
const ALL_TABS = [...PRIMARY_TABS, ...MORE_TABS] as const;
type Tab = (typeof ALL_TABS)[number];

type MemoryTypeFilter = "all" | MemoryType;
type StatusFilter = "all" | MemoryStatus;

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

const MEMORY_TYPES: MemoryType[] = [
  "architecture",
  "convention",
  "decision",
  "incident",
  "debugging",
  "environment",
  "domain_fact",
  "documentation",
  "plan",
  "implementation",
  "user",
  "feedback",
  "project",
  "reference",
];

export default function App() {
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
  const [queryHistory, setQueryHistory] = useState<string[]>([]);
  const [queryHistoryCursor, setQueryHistoryCursor] = useState<number | null>(null);
  const [selectedQueryMemoryLoading, setSelectedQueryMemoryLoading] = useState(false);
  const [selectedQueryMemoryError, setSelectedQueryMemoryError] = useState<string | null>(null);
  const [activities, setActivities] = useState<ActivityEvent[]>([]);
  const [selectedActivityIndex, setSelectedActivityIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState("Connecting to Memory Layer...");
  const [connectionState, setConnectionState] = useState<"connecting" | "live" | "offline">("connecting");
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
      .catch((error: Error) => setStatusMessage(error.message));
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

  const selectedEmbeddingBackend = embeddingBackends?.backends[selectedEmbeddingIndex] ?? null;
  const embeddingBusy = embeddingLoading || embeddingOperation !== null;

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

  // Keyboard shortcuts
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      const inInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";

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
  }, [embeddingBusy, project, selectedEmbeddingBackend, tab]);

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
    setQueryHistory((current) => {
      const withoutDuplicateTail = current[current.length - 1] === trimmed ? current : [...current, trimmed];
      return withoutDuplicateTail.slice(-50);
    });
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
      setQueryRoundtripMs(Math.round(performance.now() - started));
      setStatusMessage(`Query returned ${response.results.length} memories in ${Math.round(performance.now() - started)} ms.`);
      setTab("query");
    } catch (error) {
      const message = (error as Error).message;
      setQueryError(message);
      setQueryRoundtripMs(Math.round(performance.now() - started));
      setStatusMessage(message);
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
      setQueryText(queryHistory[next]);
      setStatusMessage(`Loaded query history item ${next + 1}/${queryHistory.length}.`);
    }
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
  const selectedAgent = agentSnapshot?.sessions[selectedAgentIndex] ?? null;
  const activeProposal = proposals[selectedProposalIndex] ?? null;

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
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
          <label>
            Repo root
            <input
              placeholder="Auto"
              value={repoRootInput}
              onChange={(event) => setRepoRootInput(event.target.value)}
            />
          </label>
          <button type="submit">Load</button>
        </form>
      </header>

      <section className="status-strip">
        <span className={`status-pill status-${connectionState}`}>{connectionState}</span>
        <span><strong>{overview.project}</strong></span>
        <span>service {overview.service_status}</span>
        <span>db {overview.database_status}</span>
        <span>{overview.memory_entries_total} memories</span>
        <span>{overview.raw_captures_total} captures</span>
        <span>{overview.watchers?.active_count ?? 0} watchers</span>
        <span>mem-service {serviceVersion}</span>
      </section>

      <nav className="tabs">
        {PRIMARY_TABS.map((name, i) => (
          <button
            key={name}
            className={tab === name ? "tab-active" : ""}
            onClick={() => setTab(name)}
            type="button"
            title={`${i + 1}`}
          >
            {name}
          </button>
        ))}
        <select className="more-select" value={MORE_TABS.includes(tab as (typeof MORE_TABS)[number]) ? tab : ""} onChange={(event) => event.target.value && setTab(event.target.value as Tab)}>
          <option value="">More</option>
          {MORE_TABS.map((name) => (
            <option key={name} value={name}>{name}</option>
          ))}
        </select>
      </nav>

      {/* ── Memories ── */}
      {tab === "memories" ? (
        <section className="panel-grid">
          <div className="panel">
            <div className="panel-toolbar filters-grid">
              <input ref={searchRef} placeholder="Search summary or preview (/)" value={textFilter} onChange={(e) => setTextFilter(e.target.value)} />
              <input placeholder="Filter tag" value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} />
              <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}>
                <option value="all">All statuses</option>
                <option value="active">Active</option>
                <option value="archived">Archived</option>
              </select>
              <select value={typeFilter} onChange={(e) => setTypeFilter(e.target.value as MemoryTypeFilter)}>
                <option value="all">All types</option>
                {MEMORY_TYPES.map((memoryType) => (
                  <option key={memoryType} value={memoryType}>{memoryType}</option>
                ))}
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
            {selectedHistory ? (
              <>
                <div className="detail-header">
                  <div>
                    <h2>Version history</h2>
                    <p>{selectedHistory.project} · canonical {selectedHistory.canonical_id} · {selectedHistory.versions.length} version(s)</p>
                  </div>
                  <button onClick={() => setSelectedHistory(null)} type="button">Hide history</button>
                </div>
                {selectedHistory.versions.map((version) => (
                  <section className="detail-section version-card" key={version.id}>
                    <h3>v{version.version_no} {version.is_tombstone ? "(tombstone)" : ""}</h3>
                    <p>{version.memory_type} · {version.status} · {formatDateTime(version.updated_at)}</p>
                    <strong>{version.summary}</strong>
                    {version.is_tombstone ? <p>Memory was deleted at this version.</p> : <RichText text={version.canonical_text} />}
                  </section>
                ))}
              </>
            ) : selectedMemory ? (
              <>
                <div className="detail-header">
                  <div>
                    <h2>{selectedMemory.summary}</h2>
                    <p>{selectedMemory.memory_type} · {selectedMemory.status} · confidence {selectedMemory.confidence.toFixed(2)} · importance {selectedMemory.importance} · v{selectedMemory.version_no}</p>
                  </div>
                  <div className="proposal-actions">
                    <button onClick={() => void handleLoadHistory(selectedMemory.id)} type="button">History</button>
                    <button className="danger" onClick={() => void handleDelete(selectedMemory.id)} type="button">Delete</button>
                  </div>
                </div>
                <section className="detail-section">
                  <h3>Embeddings</h3>
                  {selectedMemory.embedding_spaces.length ? (
                    selectedMemory.embedding_spaces.map((space) => (
                      <div key={`${space.provider}-${space.model}-${space.base_url}`} className="metric-row">
                        <span>{space.provider} / {space.model}</span>
                        <strong>{space.chunk_count} chunk(s){space.last_updated ? ` · ${formatDateTime(space.last_updated)}` : ""}</strong>
                      </div>
                    ))
                  ) : (
                    <p className="muted">No embeddings for this memory yet. Run Re-embed for this project to populate the active embedding space.</p>
                  )}
                </section>
                <section className="detail-section">
                  <h3>Canonical text</h3>
                  <RichText text={selectedMemory.canonical_text} />
                </section>
                <section className="detail-section">
                  <h3>Tags</h3>
                  <div className="tag-wrap">{selectedMemory.tags.map((t) => <span key={t} className="tag">{t}</span>)}</div>
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

      {/* ── Agents ── */}
      {tab === "agents" ? (
        <section className="panel-grid">
          <div className="panel">
            <div className="list-view">
              {agentSnapshot?.sessions.length ? (
                agentSnapshot.sessions.map((session, index) => (
                  <button
                    key={session.session_id}
                    type="button"
                    className={`list-item ${selectedAgentIndex === index ? "selected" : ""}`}
                    onClick={() => setSelectedAgentIndex(index)}
                  >
                    <div>
                      <strong>{session.project_name}</strong>
                      <p>{session.current_tasks.join(", ") || "idle"}</p>
                    </div>
                    <div className="meta-stack">
                      <span className="badge">{session.agent_cli}</span>
                      <span className={`status-pill status-${session.status}`}>{session.status}</span>
                      <span>{session.model}</span>
                      <span>{formatElapsed(session.started_at)}</span>
                    </div>
                  </button>
                ))
              ) : (
                <p className="muted">No agent sessions detected.</p>
              )}
            </div>
          </div>
          <div className="panel detail-scroll">
            {selectedAgent ? (
              <>
                <h2>{selectedAgent.project_name}</h2>
                <p className={`status-pill status-${selectedAgent.status}`}>{selectedAgent.status}</p>
                <Metric label="Collected" value={formatDateTime(agentSnapshot?.collected_at)} />
                <Metric label="Agent" value={`${selectedAgent.agent_cli} ${selectedAgent.version}`} />
                <Metric label="Session" value={selectedAgent.session_id} />
                <Metric label="PID" value={String(selectedAgent.pid)} />
                <Metric label="Model" value={selectedAgent.model} />
                <Metric label="Context" value={`${selectedAgent.context_percent.toFixed(1)}%`} />
                <Metric label="Turns" value={String(selectedAgent.turn_count)} />
                <Metric label="Tokens" value={`${formatTokens(selectedAgent.total_input_tokens)} in / ${formatTokens(selectedAgent.total_output_tokens)} out`} />
                <Metric label="Cache" value={`${formatTokens(selectedAgent.total_cache_read)} read / ${formatTokens(selectedAgent.total_cache_create)} create`} />
                <Metric label="Memory" value={`${selectedAgent.mem_mb} MB`} />
                <Metric label="Working directory" value={selectedAgent.cwd} />
                <Metric label="Git" value={`${selectedAgent.git_branch || "n/a"} (+${selectedAgent.git_added} ~${selectedAgent.git_modified})`} />
                <Metric label="Prompt" value={selectedAgent.initial_prompt || "n/a"} />
                <Metric label="Current tasks" value={selectedAgent.current_tasks.join(", ") || "none"} />
                {selectedAgent.subagents.length > 0 && (
                  <section className="detail-section">
                    <h3>Subagents</h3>
                    {selectedAgent.subagents.map((sa) => (
                      <div key={sa.name} className="metric-row">
                        <span>{sa.name} ({sa.status})</span>
                        <strong>{formatTokens(sa.tokens)} tokens</strong>
                      </div>
                    ))}
                  </section>
                )}
                {selectedAgent.children.length > 0 && (
                  <section className="detail-section">
                    <h3>Child processes</h3>
                    {selectedAgent.children.map((ch) => (
                      <div key={ch.pid} className="metric-row">
                        <span>PID {ch.pid}: {ch.command}</span>
                        <strong>{ch.port ? `port ${ch.port}` : `${ch.mem_kb} KB`}</strong>
                      </div>
                    ))}
                  </section>
                )}
                {(agentSnapshot?.orphan_ports.length ?? 0) > 0 && (
                  <section className="detail-section">
                    <h3>Orphan ports</h3>
                    {agentSnapshot!.orphan_ports.map((op) => (
                      <div key={`${op.pid}-${op.port}`} className="metric-row">
                        <span>:{op.port} (PID {op.pid}) {op.command}</span>
                        <strong>{op.project_name}</strong>
                      </div>
                    ))}
                  </section>
                )}
                {(agentSnapshot?.rate_limits.length ?? 0) > 0 && (
                  <section className="detail-section">
                    <h3>Rate limits</h3>
                    {agentSnapshot!.rate_limits.map((limit) => (
                      <div key={limit.source} className="metric-row">
                        <span>{limit.source}</span>
                        <strong>
                          5h {formatPercent(limit.five_hour_pct)} / 7d {formatPercent(limit.seven_day_pct)}
                        </strong>
                        <span className="muted">
                          resets {formatEpochSeconds(limit.five_hour_resets_at)} / {formatEpochSeconds(limit.seven_day_resets_at)}
                        </span>
                      </div>
                    ))}
                  </section>
                )}
              </>
            ) : (
              <p className="muted">Select an agent session to inspect its details.</p>
            )}
          </div>
        </section>
      ) : null}

      {/* ── Query ── */}
      {tab === "query" ? (
        <section className="panel-stack">
          <form className="panel" onSubmit={handleQuerySubmit}>
            <div className="panel-toolbar">
              <input
                ref={queryRef}
                className="query-input"
                placeholder="Ask what the project knows... (?)"
                value={queryText}
                onChange={(event) => setQueryText(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    applyQueryHistory(-1);
                  } else if (event.key === "ArrowDown") {
                    event.preventDefault();
                    applyQueryHistory(1);
                  } else {
                    setQueryHistoryCursor(null);
                  }
                }}
              />
              <button type="submit" disabled={queryLoading}>{queryLoading ? "Searching..." : "Query"}</button>
            </div>
            {queryLoading ? (
              <div className="query-summary">
                <p>Searching "{queryText.trim()}"...</p>
                <p className="muted">Previous results remain visible until the new search finishes.</p>
              </div>
            ) : null}
            {queryError ? (
              <div className="query-summary warning-list">
                <strong>Query failed</strong>
                <p>{queryError}</p>
              </div>
            ) : null}
            {queryResponse ? (
              <div className="query-summary">
                <p>{queryResponse.answer}</p>
                <div className="stats-row">
                  <span>{queryResponse.answer_generation.method}</span>
                  <span>citations {formatCitationNumbers(queryResponse.answer_generation.cited_result_numbers)}</span>
                  <span>answer {queryResponse.answer_generation.duration_ms} ms</span>
                  <span>roundtrip {queryRoundtripMs ?? "n/a"} ms</span>
                  <span>confidence {queryResponse.confidence.toFixed(2)}</span>
                  <span>{queryResponse.insufficient_evidence ? "insufficient evidence" : "sufficient evidence"}</span>
                  <span>lexical {queryResponse.diagnostics.lexical_candidates}</span>
                  <span>semantic {queryResponse.diagnostics.semantic_candidates}</span>
                  <span>merged {queryResponse.diagnostics.merged_candidates}</span>
                  <span>returned {queryResponse.diagnostics.returned_results}</span>
                  <span>relation {queryResponse.diagnostics.relation_augmented_candidates}</span>
                  <span>rerank {queryResponse.diagnostics.rerank_duration_ms} ms</span>
                  <span>total {queryResponse.diagnostics.total_duration_ms} ms</span>
                </div>
                {queryResponse.answer_generation.fallback_reason ? (
                  <p className="muted">Fallback: {queryResponse.answer_generation.fallback_reason}</p>
                ) : null}
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
                      <span className="badge">#{index + 1}</span>
                      <span className="badge">{result.memory_type}</span>
                      <span className="badge">{result.match_kind}</span>
                      {queryResponse?.answer_generation.cited_result_numbers.includes(index + 1) ? <span className="badge badge-active">cited</span> : null}
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
                      <p>{activeQueryResult.memory_type} · {activeQueryResult.match_kind} · score {activeQueryResult.score.toFixed(2)}</p>
                    </div>
                    <button className="danger" onClick={() => void handleDelete(activeQueryResult.memory_id)} type="button">Delete</button>
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
                      <span>phrases {activeQueryResult.debug.exact_phrase_matches}</span>
                      <span>tags {activeQueryResult.debug.tag_match_count}</span>
                      <span>paths {activeQueryResult.debug.path_match_count}</span>
                      <span>importance {activeQueryResult.debug.importance}</span>
                      <span>memory confidence {formatNumber(activeQueryResult.debug.memory_confidence)}</span>
                      <span>recency {formatNumber(activeQueryResult.debug.recency_boost)}</span>
                    </div>
                  </section>
                  <section className="detail-section">
                    <h3>Tags</h3>
                    {activeQueryResult.tags.length ? (
                      <div className="tag-wrap">{activeQueryResult.tags.map((tag) => <span key={tag} className="tag">{tag}</span>)}</div>
                    ) : (
                      <p className="muted">No tags on this result.</p>
                    )}
                  </section>
                  {activeQueryResult.sources.length ? (
                    <section className="detail-section">
                      <h3>Sources</h3>
                      {activeQueryResult.sources.map((source, index) => (
                        <div key={`${source.source_kind}-${source.file_path ?? source.git_commit ?? index}`} className="source-card">
                          <strong>{source.source_kind}</strong>
                          <p>{source.file_path ?? source.git_commit ?? "<no path>"}</p>
                          {source.excerpt ? <pre>{source.excerpt}</pre> : null}
                        </div>
                      ))}
                    </section>
                  ) : null}
                  {selectedQueryMemoryLoading ? <p className="muted">Loading selected memory detail...</p> : null}
                  {selectedQueryMemoryError ? <p className="warning-list">Detail unavailable: {selectedQueryMemoryError}</p> : null}
                  {selectedQueryMemory ? (
                    <>
                      <section className="detail-section">
                        <h3>Memory detail</h3>
                        <RichText text={selectedQueryMemory.canonical_text} />
                      </section>
                      <section className="detail-section">
                        <h3>Related memories</h3>
                        {selectedQueryMemory.related_memories.length ? (
                          selectedQueryMemory.related_memories.map((related) => (
                            <div key={`${related.relation_type}-${related.memory_id}`} className="relation-row">
                              <span className="badge">{related.relation_type}</span>
                              <span>{related.summary}</span>
                              <span className="muted">{related.memory_type} · {related.confidence.toFixed(2)}</span>
                            </div>
                          ))
                        ) : (
                          <p className="muted">No related memories recorded.</p>
                        )}
                      </section>
                    </>
                  ) : null}
                </>
              ) : (
                <p className="muted">Select a returned memory to inspect its ranking details.</p>
              )}
            </div>
          </section>
        </section>
      ) : null}

      {/* ── Activity ── */}
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
                    <p className="muted">{activityTokenLabel(event)} · {activityDurationLabel(event)}</p>
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
                <p className="muted">
                  {formatDateTime(activeActivity.recorded_at)} · {activityTokenLabel(activeActivity)} · {activityDurationLabel(activeActivity)}
                </p>
                <ActivityDetail event={activeActivity} />
              </>
            ) : (
              <p className="muted">Keep this page open while queries, captures, curation runs, and deletions happen.</p>
            )}
          </div>
        </section>
      ) : null}

      {/* ── Project ── */}
      {tab === "project" ? (
        <section className="panel-stack">
          <div className="panel actions-row">
            <button onClick={() => void refreshProject(project)} type="button">Refresh</button>
            <button onClick={() => void runProjectAction("curate")} type="button">Curate</button>
            <button onClick={() => void runProjectAction("reindex")} type="button">Reindex</button>
            <button onClick={() => void runProjectAction("reembed")} type="button">Re-embed</button>
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
              <Metric label="Embeddings" value={`${overview.embedding_chunks_total} chunks / ${overview.fresh_embedding_chunks} active-space / ${overview.stale_embedding_chunks} other-space only / ${overview.missing_embedding_chunks} missing active-space`} />
              <Metric label="Embedding spaces" value={`${overview.embedding_spaces_total} stored space(s)`} />
              <Metric label="Active embedding" value={overview.active_embedding_model ? `${overview.active_embedding_provider} / ${overview.active_embedding_model}` : "disabled"} />
              <Metric label="Curation policy" value={`${replacementPolicy?.replacement_policy ?? "unknown"} / ${overview.pending_replacement_proposals} pending (Review tab)`} />
              <Metric label="Tasks / Sessions / Runs" value={`${overview.tasks_total} / ${overview.sessions_total} / ${overview.curation_runs_total}`} />
              <Metric label="Last memory" value={formatDateTime(overview.last_memory_at)} />
              <Metric label="Last curation" value={formatDateTime(overview.last_curation_at)} />
              <Metric label="Last capture" value={formatDateTime(overview.last_capture_at)} />
              <Metric
                label="Automation"
                value={
                  overview.automation
                    ? `${overview.automation.mode} · dirty ${overview.automation.dirty_file_count ?? 0} · notes ${overview.automation.pending_note_count ?? 0} · ${overview.automation.repo_root}`
                    : "not configured"
                }
              />
              <Metric label="Watchers" value={`${overview.watchers?.active_count ?? 0} healthy / ${overview.watchers?.unhealthy_count ?? 0} unhealthy`} />
            </div>
            <div className="panel">
              <h2>Memory types</h2>
              <KeyValueList items={overview.memory_type_breakdown.map((item) => [item.memory_type, String(item.count)])} empty="No memory type data." />
              <h2 style={{ marginTop: "1rem" }}>Source kinds</h2>
              <KeyValueList items={overview.source_kind_breakdown.map((item) => [item.source_kind, String(item.count)])} empty="No source kind data." />
            </div>
            <div className="panel">
              <h2>Top tags</h2>
              <KeyValueList items={overview.top_tags.map((item) => [item.name, String(item.count)])} empty="No tags yet." />
            </div>
            <div className="panel">
              <h2>Top files</h2>
              <KeyValueList items={overview.top_files.map((item) => [item.name, String(item.count)])} empty="No file provenance yet." />
            </div>
            <div className="panel">
              <h2>Recent activity</h2>
              {activities.length ? (
                activities.slice(0, 6).map((event, index) => (
                  <button
                    key={`${event.recorded_at}-${event.kind}-${index}`}
                    type="button"
                    className="activity-row-button"
                    onClick={() => {
                      setSelectedActivityIndex(index);
                      setTab("activity");
                    }}
                  >
                    <span className="muted">{formatDateTime(event.recorded_at)}</span>
                    <strong>{event.kind}</strong>
                    <span>{event.summary}</span>
                  </button>
                ))
              ) : (
                <p className="muted">No recent activity in this browser session.</p>
              )}
            </div>
          </section>
          {proposals.length > 0 && (
            <div className="panel">
              <h2>Replacement proposals ({proposals.length})</h2>
              {proposals.map((proposal) => (
                <div key={proposal.id} className="proposal-card">
                  <p><strong>Target:</strong> {proposal.target_summary}</p>
                  <p><strong>Candidate:</strong> {proposal.candidate_summary}</p>
                  <p className="muted">
                    {proposal.candidate_memory_type} · score {proposal.score} · {proposal.policy}
                    {proposal.reasons.length > 0 && ` · ${proposal.reasons.join(", ")}`}
                  </p>
                  <div className="proposal-actions">
                    <button className="approve-btn" onClick={() => void handleApproveProposal(proposal.id)} type="button">Approve</button>
                    <button className="reject-btn" onClick={() => void handleRejectProposal(proposal.id)} type="button">Reject</button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      ) : null}

      {/* ── Review ── */}
      {tab === "review" ? (
        <section className="panel-grid">
          <div className="panel detail-scroll">
            <div className="detail-header">
              <div>
                <h2>Curation review</h2>
                <p>
                  Policy {replacementPolicy?.replacement_policy ?? "unknown"} · {proposals.length} pending
                  {proposals.length ? ` · selected ${selectedProposalIndex + 1}/${proposals.length}` : ""}
                  {effectiveRepoRoot ? ` · ${effectiveRepoRoot}` : ""}
                </p>
              </div>
              <div className="proposal-actions">
                <button onClick={() => void refreshReview()} type="button">Refresh</button>
                <button onClick={() => void handleCyclePolicy()} type="button" disabled={!effectiveRepoRoot}>Cycle policy</button>
              </div>
            </div>
            {!effectiveRepoRoot ? (
              <p className="warning-list">Set a repo root to change policy. Approve/reject still works for queued proposals.</p>
            ) : null}
            <div className="list-view">
              {proposals.length ? proposals.map((proposal) => (
                <button
                  key={proposal.id}
                  type="button"
                  className={`list-item ${activeProposal?.id === proposal.id ? "selected" : ""}`}
                  onClick={() => setSelectedProposalIndex(proposals.findIndex((item) => item.id === proposal.id))}
                >
                  <div>
                    <strong>{proposal.target_summary}</strong>
                    <p>{proposal.candidate_summary}</p>
                  </div>
                  <div className="meta-stack">
                    <span className="badge">{proposal.candidate_memory_type}</span>
                    <span>{proposal.score}</span>
                  </div>
                </button>
              )) : <p className="muted">No pending replacement proposals.</p>}
            </div>
          </div>
          <div className="panel detail-scroll">
            {activeProposal ? (
              <>
                <h2>Proposal detail</h2>
                <section className="detail-section">
                  <h3>Target</h3>
                  <p>{activeProposal.target_summary}</p>
                </section>
                <section className="detail-section">
                  <h3>Candidate</h3>
                  <p><strong>{activeProposal.candidate_summary}</strong></p>
                  <RichText text={activeProposal.candidate_canonical_text} />
                </section>
                <div className="stats-row">
                  <span className="badge">{activeProposal.candidate_memory_type}</span>
                  <span className="badge">{activeProposal.policy}</span>
                  <span>score {activeProposal.score}</span>
                  {activeProposal.reasons.map((reason) => <span key={reason}>{reason}</span>)}
                </div>
                <div className="proposal-actions">
                  <button className="approve-btn" onClick={() => void handleApproveProposal(activeProposal.id)} type="button">Approve</button>
                  <button className="reject-btn" onClick={() => void handleRejectProposal(activeProposal.id)} type="button">Reject</button>
                </div>
              </>
            ) : (
              <p className="muted">Queued ambiguous curation candidates will appear here.</p>
            )}
          </div>
        </section>
      ) : null}

      {/* ── Watchers ── */}
      {tab === "watchers" ? (
        <section className="panel-stack">
          <div className="panel">
            <h2>Watcher presence</h2>
            {overview.watchers ? (
              <>
                <Metric label="Active watchers" value={String(overview.watchers.active_count)} />
                <Metric label="Unhealthy watchers" value={String(overview.watchers.unhealthy_count)} />
                <Metric label="Stale after" value={`${overview.watchers.stale_after_seconds}s`} />
                <Metric label="Last heartbeat" value={formatDateTime(overview.watchers.last_heartbeat_at)} />
              </>
            ) : (
              <p className="muted">No watcher presence data.</p>
            )}
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
                    <span className={`badge ${watcher.health === "healthy" ? "badge-active" : "badge-archived"}`}>{watcher.health}</span>
                    <span>{watcher.managed_by_service ? "managed" : "manual"}</span>
                    <span>started {formatDateTime(watcher.started_at)}</span>
                    <span>{formatDateTime(watcher.last_heartbeat_at)}</span>
                    <span>restarts {watcher.restart_attempt_count}</span>
                    <span className="muted">{watcher.watcher_id}</span>
                  </div>
                  <p className="muted">Host service {watcher.host_service_id}</p>
                  {watcher.agent_session_id ? (
                    <p className="muted">{watcher.agent_cli} session {watcher.agent_session_id} · agent pid {watcher.agent_pid ?? "n/a"}</p>
                  ) : null}
                  {watcher.last_restart_attempt_at ? (
                    <p className="muted">Last restart attempt {formatDateTime(watcher.last_restart_attempt_at)}</p>
                  ) : null}
                </div>
              ))
            ) : (
              <p className="muted">
                No watcher presence reported. Start one with{" "}
                <code>memory watcher run --project {project}</code> or enable the watcher manager with{" "}
                <code>memory watcher manager enable</code>.
              </p>
            )}
          </div>
        </section>
      ) : null}

      {/* ── Embeddings ── */}
      {tab === "embeddings" ? (
        <section className="panel-stack">
          <div className="panel actions-row">
            <button onClick={() => void refreshEmbeddings()} type="button" disabled={embeddingBusy}>
              {embeddingLoading ? "Refreshing..." : "Refresh"}
            </button>
            <button onClick={() => void runProjectAction("reindex")} type="button" disabled={embeddingBusy}>Reindex all</button>
            <button onClick={() => void runProjectAction("reembed")} type="button" disabled={embeddingBusy}>Re-embed all</button>
          </div>
          <section className="panel-grid">
            <div className="panel">
              <h2>Embedding backends</h2>
              <div className="stats-row">
                <span>active {embeddingBackends?.active ?? "none"}</span>
                <span>create {selectedEmbeddingBackend ? `${selectedEmbeddingBackend.create_enabled ? "on" : "off"} for ${selectedEmbeddingBackend.name}` : "unknown"}</span>
                <span>{embeddingBackends?.backends.length ?? 0} configured</span>
                <span>{embeddingBackends?.backends.filter((backend) => backend.ready).length ?? 0} ready</span>
                <span>{embeddingBackends?.backends.filter((backend) => !backend.ready).length ?? 0} not ready</span>
              </div>
              <p className="muted">
                Status: {embeddingOperation ? `${embeddingOperation}...` : embeddingLoading ? "refreshing..." : "idle"}
              </p>
              <div className="list-view">
                {(embeddingBackends?.backends ?? []).map((backend, index) => (
                  <button
                    key={backend.name}
                    type="button"
                    className={`list-item ${selectedEmbeddingIndex === index ? "selected" : ""}`}
                    onClick={() => setSelectedEmbeddingIndex(index)}
                  >
                    <div>
                      <strong>{backend.active ? "* " : ""}{backend.name}</strong>
                      <p>{backend.provider} · {backend.model}{backend.base_url ? ` · ${backend.base_url}` : ""}</p>
                    </div>
                    <div className="meta-stack">
                      <span className={`badge ${backend.ready ? "badge-active" : "badge-archived"}`}>{backend.ready ? "ready" : "not ready"}</span>
                      <span className={`badge ${backend.create_enabled ? "badge-active" : "badge-archived"}`}>create {backend.create_enabled ? "on" : "off"}</span>
                      <span>{backend.project_chunk_count ?? 0} chunks</span>
                      <span>{backend.project_memory_count ?? 0} memories</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
            <div className="panel detail-scroll">
              {selectedEmbeddingBackend ? (
                <>
                  <h2>{selectedEmbeddingBackend.name}</h2>
                  <Metric label="Provider" value={selectedEmbeddingBackend.provider} />
                  <Metric label="Model" value={selectedEmbeddingBackend.model || "n/a"} />
                  <Metric label="Base URL" value={selectedEmbeddingBackend.base_url || "default"} />
                  <Metric label="Coverage" value={`${selectedEmbeddingBackend.project_chunk_count ?? 0} chunks / ${selectedEmbeddingBackend.project_memory_count ?? 0} memories`} />
                  <Metric label="Status" value={selectedEmbeddingBackend.ready ? "ready" : "not ready"} />
                  <Metric label="Search" value={selectedEmbeddingBackend.active ? "active" : "inactive"} />
                  <Metric label="Automatic creation" value={selectedEmbeddingBackend.create_enabled ? "on" : "off"} />
                  <div className="proposal-actions">
                    <button
                      onClick={() => void handleToggleEmbeddingSearch(selectedEmbeddingBackend)}
                      type="button"
                      disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                    >
                      {selectedEmbeddingBackend.active ? "Turn off search" : "Activate"}
                    </button>
                    <button
                      onClick={() => void handleToggleEmbeddingCreation(selectedEmbeddingBackend)}
                      type="button"
                      disabled={embeddingBusy}
                    >
                      {selectedEmbeddingBackend.create_enabled ? "Disable automatic creation" : "Enable automatic creation"}
                    </button>
                    <button
                      onClick={() => void handleReembedEmbeddingBackend(selectedEmbeddingBackend)}
                      type="button"
                      disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                    >
                      Create embeddings
                    </button>
                    <button
                      onClick={() => void handleReindexEmbeddingBackend(selectedEmbeddingBackend)}
                      type="button"
                      disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                    >
                      Reindex
                    </button>
                  </div>
                  <p className="muted">Shortcuts: Enter toggles search, c toggles automatic creation, e creates embeddings, I reindexes, r refreshes.</p>
                </>
              ) : (
                <p className="muted">No embedding backends configured.</p>
              )}
            </div>
          </section>
        </section>
      ) : null}

      {/* ── Resume ── */}
      {tab === "resume" ? (
        <section className="panel-stack">
          <div className="panel actions-row">
            <button onClick={() => void handleLoadResume()} type="button" disabled={resumeLoading}>
              {resumeLoading ? "Generating..." : "Load resume"}
            </button>
          </div>
          <div className="panel detail-scroll">
            {resumeLoading ? (
              <p className="loading-indicator">Generating project briefing...</p>
            ) : resumeData ? (
              <>
                <h2>Resume for {resumeData.project}</h2>
                <p className="muted">Generated {formatDateTime(resumeData.generated_at)}</p>

                {resumeData.checkpoint && (
                  <div className="resume-section">
                    <h3>Checkpoint</h3>
                    <p>{resumeData.checkpoint.note ?? "Checkpoint saved"}</p>
                    <p className="muted">
                      {formatDateTime(resumeData.checkpoint.marked_at)}
                      {resumeData.checkpoint.git_branch ? ` · ${resumeData.checkpoint.git_branch}` : ""}
                      {resumeData.checkpoint.git_head ? ` · ${resumeData.checkpoint.git_head.slice(0, 8)}` : ""}
                    </p>
                  </div>
                )}

                {resumeData.current_thread && (
                  <div className="resume-section">
                    <h3>Current work</h3>
                    <p>{resumeData.current_thread}</p>
                  </div>
                )}

                {resumeData.primary_next_step && (
                  <div className="resume-section">
                    <h3>Next step</h3>
                    <div className="action-card">
                      <strong>{resumeData.primary_next_step.title}</strong>
                      <p>{resumeData.primary_next_step.rationale}</p>
                      {resumeData.primary_next_step.command_hint && <code>{resumeData.primary_next_step.command_hint}</code>}
                    </div>
                  </div>
                )}

                {resumeData.secondary_next_steps.length > 0 && (
                  <div className="resume-section">
                    <h3>Other actions</h3>
                    {resumeData.secondary_next_steps.map((action) => (
                      <div key={action.title} className="action-card">
                        <strong>{action.title}</strong>
                        <p>{action.rationale}</p>
                        {action.command_hint && <code>{action.command_hint}</code>}
                      </div>
                    ))}
                  </div>
                )}

                {resumeData.change_summary.length > 0 && (
                  <div className="resume-section">
                    <h3>What changed</h3>
                    <ul>{resumeData.change_summary.map((item) => <li key={item}>{item}</li>)}</ul>
                  </div>
                )}

                {resumeData.attention_items.length > 0 && (
                  <div className="resume-section">
                    <h3>Needs attention</h3>
                    <ul>{resumeData.attention_items.map((item) => <li key={item}>{item}</li>)}</ul>
                  </div>
                )}

                {resumeData.context_items.length > 0 && (
                  <div className="resume-section">
                    <h3>Keep in mind</h3>
                    {resumeData.context_items.map((mem) => (
                      <div key={mem.id} className="metric-row">
                        <span className="badge">{mem.memory_type}</span>
                        <span>{mem.summary}</span>
                      </div>
                    ))}
                  </div>
                )}

                {resumeData.durable_context.length > 0 && (
                  <div className="resume-section">
                    <h3>Durable context</h3>
                    {resumeData.durable_context.map((mem) => (
                      <div key={mem.id} className="metric-row">
                        <span className="badge">{mem.memory_type}</span>
                        <span>{mem.summary}</span>
                      </div>
                    ))}
                  </div>
                )}

                {resumeData.timeline.length > 0 && (
                  <div className="resume-section">
                    <h3>Timeline</h3>
                    {resumeData.timeline.map((event, i) => (
                      <div key={`${event.recorded_at}-${i}`} className="metric-row">
                        <span className="muted">{formatDateTime(event.recorded_at)}</span>
                        <span>{event.summary}</span>
                      </div>
                    ))}
                  </div>
                )}

                {resumeData.warnings.length > 0 && (
                  <div className="resume-section">
                    <h3>Warnings</h3>
                    <ul className="warning-list">{resumeData.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
                  </div>
                )}

                {resumeData.actions.length > 0 && (
                  <div className="resume-section">
                    <h3>All suggested next actions</h3>
                    {resumeData.actions.map((action) => (
                      <div key={`${action.title}-${action.rationale}`} className="action-card">
                        <strong>{action.title}</strong>
                        <p>{action.rationale}</p>
                        {action.command_hint && <code>{action.command_hint}</code>}
                      </div>
                    ))}
                  </div>
                )}

                {resumeData.commits.length > 0 && (
                  <div className="resume-section">
                    <h3>Recent commits</h3>
                    {resumeData.commits.map((commit) => (
                      <div key={commit.hash} className="metric-row">
                        <span className="badge">{commit.short_hash}</span>
                        <span>{commit.subject}</span>
                        <span className="muted">{formatDateTime(commit.committed_at)}</span>
                      </div>
                    ))}
                  </div>
                )}

                <div className="resume-section">
                  <h3>Briefing</h3>
                  <RichText text={resumeData.briefing} />
                </div>
              </>
            ) : (
              <p className="muted">Click "Load resume" to generate a project briefing with next steps and context.</p>
            )}
          </div>
        </section>
      ) : null}

      {/* ── Bundles ── */}
      {tab === "bundles" ? (
        <section className="panel-grid">
          <div className="panel detail-scroll">
            <h2>Export bundle</h2>
            <label><input type="checkbox" checked={bundleOptions.include_archived} onChange={(event) => setBundleOptions((current) => ({ ...current, include_archived: event.target.checked }))} /> Include archived memories</label>
            <label><input type="checkbox" checked={bundleOptions.include_tags} onChange={(event) => setBundleOptions((current) => ({ ...current, include_tags: event.target.checked }))} /> Include tags</label>
            <label><input type="checkbox" checked={bundleOptions.include_relations} onChange={(event) => setBundleOptions((current) => ({ ...current, include_relations: event.target.checked }))} /> Include relations</label>
            <label><input type="checkbox" checked={bundleOptions.include_source_file_paths} onChange={(event) => setBundleOptions((current) => ({ ...current, include_source_file_paths: event.target.checked }))} /> Include source file paths</label>
            <label><input type="checkbox" checked={bundleOptions.include_git_commits} onChange={(event) => setBundleOptions((current) => ({ ...current, include_git_commits: event.target.checked }))} /> Include git commit hashes</label>
            <label><input type="checkbox" checked={bundleOptions.include_source_excerpts} onChange={(event) => setBundleOptions((current) => ({ ...current, include_source_excerpts: event.target.checked }))} /> Include source excerpts</label>
            <div className="actions-row">
              <button onClick={() => void handlePreviewExport()} type="button">Preview export</button>
              <button onClick={() => void handleDownloadExport()} type="button">Download bundle</button>
            </div>
            {exportPreview ? (
              <>
                <p className="muted">{exportPreview.memory_count} memories · {exportPreview.relation_count} relations · {exportPreview.warning_count} warnings</p>
                <pre className="code-block">{exportPreview.summary_markdown}</pre>
                {exportPreview.warnings.length ? (
                  <ul className="warning-list">{exportPreview.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
                ) : null}
              </>
            ) : (
              <p className="muted">Export a versioned, shareable bundle of the current project's curated memories.</p>
            )}
          </div>
          <div className="panel detail-scroll">
            <h2>Import bundle</h2>
            <input type="file" accept=".zip,.mlbundle.zip" onChange={(event) => setImportFile(event.target.files?.[0] ?? null)} />
            <div className="actions-row">
              <button onClick={() => void handlePreviewImport()} type="button">Preview import</button>
              <button onClick={() => void handleApplyImport()} type="button">Import bundle</button>
            </div>
            {importPreview ? (
              <>
                <p className="muted">{importPreview.memory_count} memories · {importPreview.new_count} new · {importPreview.unchanged_count} unchanged · {importPreview.replacing_count} replacing</p>
                <pre className="code-block">{importPreview.summary_markdown}</pre>
                {importPreview.warnings.length ? (
                  <ul className="warning-list">{importPreview.warnings.map((warning) => <li key={warning}>{warning}</li>)}</ul>
                ) : null}
              </>
            ) : (
              <p className="muted">Upload a bundle to preview and import it into the current project.</p>
            )}
          </div>
        </section>
      ) : null}

      <footer className="statusbar">{statusMessage}</footer>
    </div>
  );

  function sendStream(request: StreamRequest, socket = wsRef.current) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
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
  if (!items.length) return <p className="muted">{empty}</p>;
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

function ActivityDetail({ event }: { event: ActivityEvent }) {
  const details = event.details;
  const eventRows: [string, string][] = [
    ["Tokens", activityTokenLabel(event)],
    ["Duration", activityDurationLabel(event)],
    ["Provider", event.provider ?? "n/a"],
    ["Model", event.model ?? "n/a"],
    ["Source", event.source ?? "n/a"],
    ["Operation", event.operation_id ?? "n/a"],
  ];
  if (event.token_usage) {
    eventRows.push(
      ["Input tokens", formatTokens(event.token_usage.input_tokens)],
      ["Output tokens", formatTokens(event.token_usage.output_tokens)],
      ["Cache read", formatTokens(event.token_usage.cache_read_tokens)],
      ["Cache write", formatTokens(event.token_usage.cache_write_tokens)],
    );
  }

  if (!details) {
    return (
      <>
        <div className="detail-section">
          <h3>Execution</h3>
          <KeyValueList items={eventRows} empty="No execution metadata recorded." />
        </div>
        <p className="muted">No structured details recorded.</p>
      </>
    );
  }
  const rows: [string, string][] = [];
  const sections: ReactNode[] = [];

  switch (details.type) {
    case "checkpoint":
      rows.push(["Marked at", formatDateTime(details.marked_at)], ["Repo root", details.repo_root], ["Note", details.note ?? "n/a"], ["Branch", details.git_branch ?? "n/a"], ["HEAD", details.git_head ?? "n/a"]);
      break;
    case "plan":
      rows.push(["Action", details.action], ["Title", details.title], ["Thread", details.thread_key], ["Completed", `${details.completed_items}/${details.total_items}`], ["Verified complete", String(details.verified_complete)], ["Source path", details.source_path ?? "n/a"]);
      if (details.remaining_items.length) {
        sections.push(<ActivityList key="remaining" title="Remaining items" items={details.remaining_items} />);
      }
      break;
    case "scan":
      rows.push(["Dry run", String(details.dry_run)], ["Candidates", String(details.candidate_count)], ["Files", String(details.files_considered)], ["Commits", String(details.commits_considered)], ["Index reused", String(details.index_reused)], ["Report", details.report_path], ["Capture", details.capture_id ?? "n/a"], ["Curate run", details.curate_run_id ?? "n/a"]);
      break;
    case "commit_sync":
      rows.push(["Imported", String(details.imported_count)], ["Updated", String(details.updated_count)], ["Received", String(details.total_received)], ["Newest", details.newest_commit ?? "n/a"], ["Oldest", details.oldest_commit ?? "n/a"]);
      break;
    case "bundle_transfer":
      rows.push(["Bundle", details.bundle_id], ["Items", String(details.item_count)], ["Source project", details.source_project ?? "n/a"]);
      break;
    case "query":
      rows.push(["Query", details.query], ["Top K", String(details.top_k)], ["Results", String(details.result_count)], ["Confidence", details.confidence.toFixed(2)], ["Insufficient evidence", String(details.insufficient_evidence)], ["Duration", `${details.total_duration_ms} ms`]);
      if (details.answer) sections.push(<ActivityText key="answer" title="Answer" text={details.answer} />);
      if (details.error) rows.push(["Error", details.error]);
      break;
    case "watcher_health":
      rows.push(["Watcher", details.watcher_id], ["Hostname", details.hostname], ["Health", details.health], ["Previous health", details.previous_health ?? "n/a"], ["Managed by service", String(details.managed_by_service)], ["Restart attempts", String(details.restart_attempt_count)], ["Recovered after attempts", details.recovered_after_restart_attempts?.toString() ?? "n/a"], ["Agent CLI", details.agent_cli ?? "n/a"], ["Agent session", details.agent_session_id ?? "n/a"], ["Agent PID", details.agent_pid?.toString() ?? "n/a"], ["Message", details.message ?? "n/a"]);
      break;
    case "memory_replacement":
      rows.push(["Old memory", details.old_memory_id], ["Old summary", details.old_summary], ["New memory", details.new_memory_id], ["New summary", details.new_summary], ["Automatic", String(details.automatic)], ["Policy", details.policy]);
      break;
    case "capture_task":
      rows.push(["Session", details.session_id], ["Task", details.task_id], ["Raw capture", details.raw_capture_id], ["Idempotency", details.idempotency_key], ["Task title", details.task_title ?? "n/a"], ["Writer", details.writer_id]);
      break;
    case "curate":
      rows.push(["Run", details.run_id], ["Input captures", String(details.input_count)], ["Output memories", String(details.output_count)], ["Replacements", String(details.replaced_count)], ["Queued proposals", String(details.proposal_count)]);
      break;
    case "reindex":
      rows.push(["Reindexed entries", String(details.reindexed_entries)]);
      break;
    case "reembed":
      rows.push(["Re-embedded chunks", String(details.reembedded_chunks)]);
      break;
    case "archive":
      rows.push(["Archived count", String(details.archived_count)], ["Max confidence", details.max_confidence.toFixed(2)], ["Max importance", String(details.max_importance)]);
      break;
    case "delete_memory":
      rows.push(["Deleted", String(details.deleted)], ["Deleted summary", details.summary]);
      break;
  }

  return (
    <>
      <div className="detail-section">
        <h3>Execution</h3>
        <KeyValueList items={eventRows} empty="No execution metadata recorded." />
      </div>
      <div className="detail-section">
        <h3>Details</h3>
        <KeyValueList items={rows} empty="No structured details recorded." />
        {sections}
      </div>
    </>
  );
}

function ActivityList({ title, items }: { title: string; items: string[] }) {
  return (
    <section className="detail-section">
      <h3>{title}</h3>
      <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul>
    </section>
  );
}

function ActivityText({ title, text }: { title: string; text: string }) {
  return (
    <section className="detail-section">
      <h3>{title}</h3>
      <RichText text={text} />
    </section>
  );
}

function RichText({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);
  const blocks: ReactNode[] = [];
  let listItems: string[] = [];
  let codeLines: string[] = [];
  let inCode = false;

  const flushList = () => {
    if (!listItems.length) return;
    const items = listItems;
    listItems = [];
    blocks.push(<ul key={`list-${blocks.length}`} className="rich-list">{items.map((item) => <li key={item}>{item}</li>)}</ul>);
  };
  const flushCode = () => {
    if (!codeLines.length) return;
    const code = codeLines.join("\n");
    codeLines = [];
    blocks.push(<pre key={`code-${blocks.length}`}>{code}</pre>);
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    if (line.trim().startsWith("```")) {
      if (inCode) {
        inCode = false;
        flushCode();
      } else {
        flushList();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      codeLines.push(line);
      continue;
    }
    if (!line.trim()) {
      flushList();
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      flushList();
      blocks.push(<h4 key={`heading-${blocks.length}`}>{heading[2]}</h4>);
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.+)$/);
    if (bullet) {
      listItems.push(bullet[1]);
      continue;
    }
    flushList();
    blocks.push(<p key={`p-${blocks.length}`}>{line}</p>);
  }
  flushList();
  flushCode();

  return <div className="rich-text">{blocks.length ? blocks : <p className="muted">No text.</p>}</div>;
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return "n/a";
  return new Date(value).toLocaleString();
}

function formatEpochSeconds(value: number | null | undefined): string {
  if (!value) return "n/a";
  return new Date(value * 1000).toLocaleString();
}

function formatNumber(value: number | null | undefined): string {
  return typeof value === "number" ? value.toFixed(2) : "0.00";
}

function formatPercent(value: number | null | undefined): string {
  return typeof value === "number" ? `${value.toFixed(0)}%` : "n/a";
}

function formatCitationNumbers(values: number[]): string {
  return values.length ? values.map((value) => `[${value}]`).join(" ") : "none";
}

function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}

function activityTokenLabel(event: ActivityEvent): string {
  return event.token_usage ? `${formatTokens(event.token_usage.total_tokens)} tokens` : "tokens not recorded";
}

function activityDurationLabel(event: ActivityEvent): string {
  return typeof event.duration_ms === "number" ? `${formatTokens(event.duration_ms)} ms` : "duration n/a";
}

function mergeActivityEvents(event: ActivityEvent, current: ActivityEvent[]): ActivityEvent[] {
  return [event, ...current.filter((item) => item.id !== event.id)];
}

function mergeActivityEventLists(primary: ActivityEvent[], secondary: ActivityEvent[]): ActivityEvent[] {
  const seen = new Set<string>();
  return [...primary, ...secondary].filter((event) => {
    if (seen.has(event.id)) return false;
    seen.add(event.id);
    return true;
  });
}

function formatElapsed(startedAtMs: number): string {
  const secs = Math.max(0, Math.floor((Date.now() - startedAtMs) / 1000));
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

function websocketUrl(): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws`;
}

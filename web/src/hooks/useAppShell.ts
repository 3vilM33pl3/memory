import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { archiveProject, curate, getHealth, getMemories, getOverview, getRuntimeStatus, reembed, reindex } from "../api";
import { useActivityController } from "../features/activity/useActivityController";
import { useAgentsController } from "../features/agents/useAgentsController";
import { useAutomationsController } from "../features/automations/useAutomationsController";
import { useBundlesController } from "../features/bundles/useBundlesController";
import { useEmbeddingsController } from "../features/embeddings/useEmbeddingsController";
import { useErrorsController } from "../features/errors/useErrorsController";
import { useMemoriesController } from "../features/memories/useMemoriesController";
import { useQueryController } from "../features/query/useQueryController";
import { useResumeController } from "../features/resume/useResumeController";
import { useReviewController } from "../features/review/useReviewController";
import { type Tab } from "../tabs";
import type { DiagnosticInfo, ProjectMemoriesResponse, ProjectOverviewResponse, RuntimeStatusResponse, StreamRequest } from "../types";
import { EMPTY_OVERVIEW } from "./defaultOverview";
import { useGlobalShortcuts } from "./useGlobalShortcuts";
import { useProjectStream } from "./useProjectStream";

export function useAppShell() {
  const [tab, setTab] = useState<Tab>("memories");
  const [project, setProject] = useState(localStorage.getItem("memory-layer.project") ?? "memory");
  const [projectInput, setProjectInput] = useState(project);
  const [repoRootInput, setRepoRootInput] = useState(localStorage.getItem("memory-layer.repoRoot") ?? "");
  const [health, setHealth] = useState<Record<string, unknown> | null>(null);
  const [overview, setOverview] = useState<ProjectOverviewResponse>({ ...EMPTY_OVERVIEW, project });
  const [projectMemories, setProjectMemories] = useState<ProjectMemoriesResponse>({ project, total: 0, items: [] });
  const [localDiagnostics, setLocalDiagnostics] = useState<DiagnosticInfo[]>([]);
  const [statusMessage, setStatusMessage] = useState("Connecting to Memory Layer...");
  const [connectionState, setConnectionState] = useState<"connecting" | "live" | "offline">("connecting");
  const [runtimeStatus, setRuntimeStatus] = useState<RuntimeStatusResponse | null>(null);
  const [skillFilter, setSkillFilter] = useState("memory-layer");
  const [helpOpen, setHelpOpen] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const queryRef = useRef<HTMLInputElement>(null);

  const sendStream = useCallback((request: StreamRequest, socket = wsRef.current) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(request));
  }, []);

  const recordLocalDiagnostic = useCallback((component: string, operation: string, message: string) => {
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
  }, []);

  const refreshProject = useCallback(async (nextProject: string) => {
    try {
      const [healthPayload, overviewPayload, memoriesPayload] = await Promise.all([
        getHealth(),
        getOverview(nextProject),
        getMemories(nextProject),
      ]);
      setHealth(healthPayload);
      setOverview(overviewPayload);
      setProjectMemories(memoriesPayload);
      setStatusMessage(`Loaded ${memoriesPayload.items.length} visible memories for ${nextProject}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }, []);

  useEffect(() => {
    localStorage.setItem("memory-layer.project", project);
  }, [project]);

  useEffect(() => {
    localStorage.setItem("memory-layer.repoRoot", repoRootInput);
  }, [repoRootInput]);

  useEffect(() => {
    void refreshProject(project);
  }, [project, refreshProject]);

  const effectiveRepoRoot = useMemo(() => {
    const manual = repoRootInput.trim();
    if (manual) return manual;
    const automationRoot = overview.automation?.repo_root?.trim();
    if (automationRoot) return automationRoot;
    const roots = Array.from(new Set((overview.watchers?.watchers ?? []).map((watcher) => watcher.repo_root).filter(Boolean)));
    return roots.length === 1 ? roots[0] : "";
  }, [overview.automation?.repo_root, overview.watchers?.watchers, repoRootInput]);

  const activity = useActivityController({
    project,
    activeTab: tab,
    setStatusMessage,
    recordLocalDiagnostic,
  });
  const memories = useMemoriesController({
    memories: projectMemories,
    setStatusMessage,
    sendStream,
  });
  const query = useQueryController({
    project,
    setTab: (next) => setTab(next),
    setStatusMessage,
    recordLocalDiagnostic,
    refreshProject,
  });
  const agents = useAgentsController({ activeTab: tab, project, effectiveRepoRoot });
  const embeddings = useEmbeddingsController({
    activeTab: tab,
    project,
    setStatusMessage,
    refreshProject,
  });
  const automations = useAutomationsController({
    activeTab: tab,
    project,
    effectiveRepoRoot,
    setStatusMessage,
    refreshProject,
  });
  const bundles = useBundlesController({
    project,
    setTab: (next) => setTab(next),
    setStatusMessage,
    refreshProject,
  });
  const resume = useResumeController({
    project,
    effectiveRepoRoot,
    setTab: (next) => setTab(next),
    setStatusMessage,
  });
  const review = useReviewController({
    activeTab: tab,
    project,
    effectiveRepoRoot,
    repoRootInput,
    setRepoRootInput,
    setStatusMessage,
    refreshProject,
  });
  const errors = useErrorsController({
    activities: activity.activities,
    localDiagnostics,
    connectionState,
  });

  useProjectStream({
    project,
    selectedMemoryId: memories.selectedMemoryId,
    wsRef,
    sendStream,
    setConnectionState,
    setStatusMessage,
    setOverview,
    setProjectMemories,
    setSelectedMemory: memories.setSelectedMemory,
    addActivityEvent: activity.addActivityEvent,
    recordLocalDiagnostic,
  });

  useEffect(() => {
    let active = true;
    const refreshRuntimeStatus = () => {
      void getRuntimeStatus(project, effectiveRepoRoot || null, skillFilter)
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
  }, [effectiveRepoRoot, project, recordLocalDiagnostic, skillFilter]);

  useGlobalShortcuts({
    tab,
    setTab,
    helpOpen,
    setHelpOpen,
    searchRef,
    queryRef,
    project,
    refreshProject,
    selectedEmbeddingBackend: embeddings.selectedEmbeddingBackend,
    embeddingBusy: embeddings.embeddingBusy,
    refreshEmbeddings: () => embeddings.refreshEmbeddings(),
    handleToggleEmbeddingSearch: embeddings.handleToggleEmbeddingSearch,
    handleToggleEmbeddingCreation: embeddings.handleToggleEmbeddingCreation,
    handleReembedEmbeddingBackend: embeddings.handleReembedEmbeddingBackend,
    handleReindexEmbeddingBackend: embeddings.handleReindexEmbeddingBackend,
  });

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
      if (tab === "embeddings") await embeddings.refreshEmbeddings(null, true);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  function applyProjectInput() {
    const next = projectInput.trim();
    if (!next) return;
    setProject(next);
  }

  const serviceVersion = typeof health?.version === "string" ? health.version : "unknown";

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
    skillFilter,
    setSkillFilter,
    serviceVersion,
    helpOpen,
    setHelpOpen,
    applyProjectInput,
    searchRef,
    queryRef,
    effectiveRepoRoot,
    refreshProject,
    runProjectAction,
    statusMessage,
    ...memories,
    ...agents,
    ...query,
    ...activity,
    ...errors,
    ...review,
    ...embeddings,
    ...automations,
    ...resume,
    ...bundles,
  };
}

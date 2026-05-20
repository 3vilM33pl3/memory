import { useEffect, useState, type FormEvent } from "react";

import { deleteMemory, getMemory, runQuery } from "../../api";
import type { MemoryEntryResponse, QueryResponse } from "../../types";

interface QueryHistoryEntry {
  question: string;
  response: QueryResponse;
  roundtripMs: number;
}

interface QueryControllerOptions {
  project: string;
  setTab: (tab: "query") => void;
  setStatusMessage: (message: string) => void;
  recordLocalDiagnostic: (component: string, operation: string, message: string) => void;
  refreshProject: (project: string) => Promise<void>;
}

export function useQueryController({
  project,
  setTab,
  setStatusMessage,
  recordLocalDiagnostic,
  refreshProject,
}: QueryControllerOptions) {
  const [queryText, setQueryText] = useState("");
  const [queryResponse, setQueryResponse] = useState<QueryResponse | null>(null);
  const [selectedQueryMemory, setSelectedQueryMemory] = useState<MemoryEntryResponse | null>(null);
  const [selectedQueryIndex, setSelectedQueryIndex] = useState(0);
  const [queryLoading, setQueryLoading] = useState(false);
  const [queryError, setQueryError] = useState<string | null>(null);
  const [queryRoundtripMs, setQueryRoundtripMs] = useState<number | null>(null);
  const [includeStale, setIncludeStale] = useState(false);
  const [queryHistory, setQueryHistory] = useState<QueryHistoryEntry[]>([]);
  const [queryHistoryCursor, setQueryHistoryCursor] = useState<number | null>(null);
  const [selectedQueryMemoryLoading, setSelectedQueryMemoryLoading] = useState(false);
  const [selectedQueryMemoryError, setSelectedQueryMemoryError] = useState<string | null>(null);

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
  }, [queryResponse, selectedQueryIndex, setStatusMessage]);

  async function handleQuerySubmit(event: FormEvent) {
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
        include_stale: includeStale,
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

  return {
    queryText,
    setQueryText,
    queryResponse,
    activeQueryResult: queryResponse?.results[selectedQueryIndex] ?? null,
    selectedQueryMemory,
    selectedQueryIndex,
    setSelectedQueryIndex,
    selectedQueryMemoryLoading,
    selectedQueryMemoryError,
    queryLoading,
    queryError,
    queryRoundtripMs,
    includeStale,
    setIncludeStale,
    handleQuerySubmit,
    applyQueryHistory,
    setQueryHistoryCursor,
    handleDelete,
  };
}

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";

import { getCodeGraph, getCodeGraphStatus } from "../../api";
import type {
  CodeGraphEdge,
  CodeGraphNode,
  CodeGraphResponse,
  CodeGraphStatusResponse,
  CodeGraphViewFilters,
} from "../../types";

type GraphSelection =
  | { kind: "node"; id: string }
  | { kind: "edge"; id: string }
  | { kind: "none" };

export interface GraphConnectionView {
  sourceNodeId: string;
  targetNodeId: string;
}

export interface GraphOpenSeed {
  run_id?: string | null;
  q?: string | null;
  file_path?: string | null;
  symbol?: string | null;
  edge_kind?: string | null;
}

export interface GraphFilterForm {
  run_id: string;
  q: string;
  file_path: string;
  symbol: string;
  edge_kind: string;
  depth: number;
  limit_nodes: number;
  limit_edges: number;
  isolate_connected: boolean;
  isolate_depth: number;
}

const DEFAULT_FILTERS: GraphFilterForm = {
  run_id: "",
  q: "",
  file_path: "",
  symbol: "",
  edge_kind: "",
  depth: 1,
  limit_nodes: 250,
  limit_edges: 500,
  isolate_connected: false,
  isolate_depth: 1,
};

interface GraphControllerOptions {
  activeTab: string;
  project: string;
  setStatusMessage: (message: string) => void;
  recordLocalDiagnostic: (component: string, operation: string, message: string) => void;
}

export function useGraphController({
  activeTab,
  project,
  setStatusMessage,
  recordLocalDiagnostic,
}: GraphControllerOptions) {
  const [graphFilters, setGraphFilters] = useState<GraphFilterForm>(DEFAULT_FILTERS);
  const [graphStatus, setGraphStatus] = useState<CodeGraphStatusResponse | null>(null);
  const [codeGraph, setCodeGraph] = useState<CodeGraphResponse | null>(null);
  const [graphLoading, setGraphLoading] = useState(false);
  const [graphError, setGraphError] = useState<string | null>(null);
  const [selectedGraphNodeId, setSelectedGraphNodeId] = useState<string | null>(null);
  const [selectedGraphEdgeId, setSelectedGraphEdgeId] = useState<string | null>(null);
  const [graphConnectionView, setGraphConnectionView] = useState<GraphConnectionView | null>(null);
  const [graphSelectionHistory, setGraphSelectionHistory] = useState<GraphSelection[]>([]);
  const [graphSelectionHistoryIndex, setGraphSelectionHistoryIndex] = useState(-1);
  const selectedGraphNodeIdRef = useRef<string | null>(null);
  const selectedGraphEdgeIdRef = useRef<string | null>(null);
  const graphSelectionHistoryRef = useRef<GraphSelection[]>([]);
  const graphSelectionHistoryIndexRef = useRef(-1);

  useEffect(() => {
    selectedGraphNodeIdRef.current = selectedGraphNodeId;
    selectedGraphEdgeIdRef.current = selectedGraphEdgeId;
  }, [selectedGraphEdgeId, selectedGraphNodeId]);

  useEffect(() => {
    graphSelectionHistoryRef.current = graphSelectionHistory;
    graphSelectionHistoryIndexRef.current = graphSelectionHistoryIndex;
  }, [graphSelectionHistory, graphSelectionHistoryIndex]);

  const applySelection = useCallback((selection: GraphSelection) => {
    setSelectedGraphNodeId(selection.kind === "node" ? selection.id : null);
    setSelectedGraphEdgeId(selection.kind === "edge" ? selection.id : null);
  }, []);

  const pushGraphSelection = useCallback(
    (selection: GraphSelection) => {
      applySelection(selection);
      setGraphSelectionHistory((currentHistory) => {
        const activeIndex =
          graphSelectionHistoryIndexRef.current >= 0 ? graphSelectionHistoryIndexRef.current : currentHistory.length - 1;
        const currentSelection = activeIndex >= 0 ? currentHistory[activeIndex] : null;
        if (currentSelection && sameGraphSelection(currentSelection, selection)) {
          return currentHistory;
        }
        const nextHistory = currentHistory.slice(0, activeIndex + 1);
        nextHistory.push(selection);
        graphSelectionHistoryRef.current = nextHistory;
        graphSelectionHistoryIndexRef.current = nextHistory.length - 1;
        setGraphSelectionHistoryIndex(nextHistory.length - 1);
        return nextHistory;
      });
    },
    [applySelection],
  );

  const loadGraph = useCallback(
    async (nextFilters = graphFilters) => {
      setGraphLoading(true);
      setGraphError(null);
      try {
        const [status, graph] = await Promise.all([
          getCodeGraphStatus(project),
          getCodeGraph(project, toApiFilters(nextFilters)),
        ]);
        setGraphStatus(status);
        setCodeGraph(graph);
        const hasSelectionHistory = graphSelectionHistoryRef.current.length > 0;
        const prunedHistory = graphSelectionHistoryRef.current.filter((selection) =>
          selectionExistsInGraph(selection, graph),
        );
        const activeSelection = currentGraphSelection(selectedGraphNodeIdRef.current, selectedGraphEdgeIdRef.current);
        let nextSelection = hasSelectionHistory && selectionExistsInGraph(activeSelection, graph)
          ? activeSelection
          : ({ kind: "node", id: graph.nodes[0]?.id ?? "" } as GraphSelection);
        if (nextSelection.kind === "node" && !nextSelection.id) {
          nextSelection = { kind: "none" };
        }
        applySelection(nextSelection);
        let nextHistory = prunedHistory;
        let nextIndex = nextHistory.findIndex((selection) => sameGraphSelection(selection, nextSelection));
        if (nextIndex < 0) {
          nextHistory = [...nextHistory, nextSelection];
          nextIndex = nextHistory.length - 1;
        }
        graphSelectionHistoryRef.current = nextHistory;
        graphSelectionHistoryIndexRef.current = nextIndex;
        setGraphSelectionHistory(nextHistory);
        setGraphSelectionHistoryIndex(nextIndex);
        setStatusMessage(
          graph.status.has_graph
            ? `Loaded ${graph.nodes.length} graph nodes and ${graph.edges.length} edges for ${project}.`
            : `No extracted code graph found for ${project}.`,
        );
      } catch (error) {
        const message = (error as Error).message;
        setGraphError(message);
        setStatusMessage(message);
        recordLocalDiagnostic("graph", "load", message);
      } finally {
        setGraphLoading(false);
      }
    },
    [applySelection, graphFilters, project, recordLocalDiagnostic, setStatusMessage],
  );

  useEffect(() => {
    if (activeTab !== "graph") return;
    void loadGraph();
  }, [activeTab, loadGraph]);

  const openGraph = useCallback(
    (seed: GraphOpenSeed = {}) => {
      setGraphConnectionView(null);
      const nextFilters: GraphFilterForm = {
        ...DEFAULT_FILTERS,
        run_id: seed.run_id?.trim() ?? "",
        q: seed.q?.trim() ?? "",
        file_path: seed.file_path?.trim() ?? "",
        symbol: seed.symbol?.trim() ?? "",
        edge_kind: seed.edge_kind?.trim() ?? "",
      };
      setGraphFilters(nextFilters);
      void loadGraph(nextFilters);
    },
    [loadGraph],
  );

  function handleGraphFilterChange(patch: Partial<GraphFilterForm>) {
    setGraphFilters((current) => ({ ...current, ...patch }));
  }

  function handleGraphSubmit(event: FormEvent) {
    event.preventDefault();
    setGraphConnectionView(null);
    void loadGraph(graphFilters);
  }

  const selectedGraphNode = useMemo<CodeGraphNode | null>(
    () => codeGraph?.nodes.find((node) => node.id === selectedGraphNodeId) ?? null,
    [codeGraph?.nodes, selectedGraphNodeId],
  );
  const selectedGraphEdge = useMemo<CodeGraphEdge | null>(
    () => codeGraph?.edges.find((edge) => edge.id === selectedGraphEdgeId) ?? null,
    [codeGraph?.edges, selectedGraphEdgeId],
  );

  const selectGraphNode = useCallback(
    (nodeId: string | null, options: { shiftKey?: boolean } = {}) => {
      if (options.shiftKey && nodeId && selectedGraphNodeId && selectedGraphNodeId !== nodeId) {
        setGraphConnectionView({ sourceNodeId: selectedGraphNodeId, targetNodeId: nodeId });
      } else {
        setGraphConnectionView(null);
      }
      pushGraphSelection(nodeId ? { kind: "node", id: nodeId } : { kind: "none" });
    },
    [pushGraphSelection, selectedGraphNodeId],
  );

  const selectGraphEdge = useCallback(
    (edgeId: string | null) => {
      setGraphConnectionView(null);
      pushGraphSelection(edgeId ? { kind: "edge", id: edgeId } : { kind: "none" });
    },
    [pushGraphSelection],
  );

  const clearGraphSelection = useCallback(() => {
    setGraphConnectionView(null);
    pushGraphSelection({ kind: "none" });
  }, [pushGraphSelection]);

  const goBackGraphSelection = useCallback(() => {
    setGraphConnectionView(null);
    setGraphSelectionHistoryIndex((currentIndex) => {
      if (currentIndex <= 0) return currentIndex;
      const nextIndex = currentIndex - 1;
      applySelection(graphSelectionHistory[nextIndex] ?? { kind: "none" });
      graphSelectionHistoryIndexRef.current = nextIndex;
      return nextIndex;
    });
  }, [applySelection, graphSelectionHistory]);

  const goForwardGraphSelection = useCallback(() => {
    setGraphConnectionView(null);
    setGraphSelectionHistoryIndex((currentIndex) => {
      if (currentIndex >= graphSelectionHistory.length - 1) return currentIndex;
      const nextIndex = currentIndex + 1;
      applySelection(graphSelectionHistory[nextIndex] ?? { kind: "none" });
      graphSelectionHistoryIndexRef.current = nextIndex;
      return nextIndex;
    });
  }, [applySelection, graphSelectionHistory]);

  return {
    graphFilters,
    graphStatus,
    codeGraph,
    graphConnectionView,
    graphLoading,
    graphError,
    selectedGraphNode,
    selectedGraphEdge,
    openGraph,
    handleGraphFilterChange,
    handleGraphSubmit,
    refreshGraph: () => {
      setGraphConnectionView(null);
      void loadGraph(graphFilters);
    },
    selectGraphNode,
    selectGraphEdge,
    clearGraphSelection,
    canGoBackGraphSelection: graphSelectionHistoryIndex > 0,
    canGoForwardGraphSelection:
      graphSelectionHistoryIndex >= 0 && graphSelectionHistoryIndex < graphSelectionHistory.length - 1,
    goBackGraphSelection,
    goForwardGraphSelection,
  };
}

function currentGraphSelection(nodeId: string | null, edgeId: string | null): GraphSelection {
  if (nodeId) return { kind: "node", id: nodeId };
  if (edgeId) return { kind: "edge", id: edgeId };
  return { kind: "none" };
}

function sameGraphSelection(left: GraphSelection, right: GraphSelection): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "none" || right.kind === "none") return true;
  return left.id === right.id;
}

function selectionExistsInGraph(selection: GraphSelection, graph: CodeGraphResponse): boolean {
  if (selection.kind === "none") return true;
  if (selection.kind === "node") return graph.nodes.some((node) => node.id === selection.id);
  return graph.edges.some((edge) => edge.id === selection.id);
}

function toApiFilters(filters: GraphFilterForm): Partial<CodeGraphViewFilters> {
  return {
    run_id: optional(filters.run_id),
    q: optional(filters.q),
    file_path: optional(filters.file_path),
    symbol: optional(filters.symbol),
    edge_kind: optional(filters.edge_kind),
    depth: filters.depth,
    limit_nodes: filters.limit_nodes,
    limit_edges: filters.limit_edges,
  };
}

function optional(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}

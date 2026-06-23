import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";

import { getCodeGraph, getCodeGraphStatus } from "../../api";
import type {
  CodeGraphEdge,
  CodeGraphNode,
  CodeGraphResponse,
  CodeGraphStatusResponse,
  CodeGraphViewFilters,
} from "../../types";

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
        setSelectedGraphNodeId((current) =>
          current && graph.nodes.some((node) => node.id === current) ? current : graph.nodes[0]?.id ?? null,
        );
        setSelectedGraphEdgeId((current) =>
          current && graph.edges.some((edge) => edge.id === current) ? current : null,
        );
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
    [graphFilters, project, recordLocalDiagnostic, setStatusMessage],
  );

  useEffect(() => {
    if (activeTab !== "graph") return;
    void loadGraph();
  }, [activeTab, loadGraph]);

  const openGraph = useCallback(
    (seed: GraphOpenSeed = {}) => {
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

  function selectGraphNode(nodeId: string | null) {
    setSelectedGraphNodeId(nodeId);
    if (nodeId) setSelectedGraphEdgeId(null);
  }

  function selectGraphEdge(edgeId: string | null) {
    setSelectedGraphEdgeId(edgeId);
    if (edgeId) setSelectedGraphNodeId(null);
  }

  return {
    graphFilters,
    graphStatus,
    codeGraph,
    graphLoading,
    graphError,
    selectedGraphNode,
    selectedGraphEdge,
    openGraph,
    handleGraphFilterChange,
    handleGraphSubmit,
    refreshGraph: () => void loadGraph(graphFilters),
    selectGraphNode,
    selectGraphEdge,
  };
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

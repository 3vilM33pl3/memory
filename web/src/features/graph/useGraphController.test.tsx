import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getCodeGraph, getCodeGraphStatus } from "../../api";
import type { CodeGraphResponse, CodeGraphStatusResponse } from "../../types";
import { useGraphController } from "./useGraphController";

vi.mock("../../api", () => ({
  getCodeGraph: vi.fn(),
  getCodeGraphStatus: vi.fn(),
}));

const mockedGetCodeGraph = vi.mocked(getCodeGraph);
const mockedGetCodeGraphStatus = vi.mocked(getCodeGraphStatus);

beforeEach(() => {
  vi.clearAllMocks();
  mockedGetCodeGraphStatus.mockResolvedValue(graphStatus);
  mockedGetCodeGraph.mockResolvedValue(graphResponse);
});

describe("useGraphController", () => {
  it("navigates node, edge, and cleared graph selections backward and forward", async () => {
    const { result } = renderGraphController();

    await waitFor(() => expect(result.current.selectedGraphNode?.id).toBe("node-a"));

    act(() => result.current.selectGraphNode("node-b"));
    expect(result.current.selectedGraphNode?.id).toBe("node-b");
    expect(result.current.canGoBackGraphSelection).toBe(true);

    act(() => result.current.selectGraphEdge("edge-ab"));
    expect(result.current.selectedGraphEdge?.id).toBe("edge-ab");

    act(() => result.current.clearGraphSelection());
    expect(result.current.selectedGraphNode).toBeNull();
    expect(result.current.selectedGraphEdge).toBeNull();

    act(() => result.current.goBackGraphSelection());
    expect(result.current.selectedGraphEdge?.id).toBe("edge-ab");
    expect(result.current.canGoForwardGraphSelection).toBe(true);

    act(() => result.current.goBackGraphSelection());
    expect(result.current.selectedGraphNode?.id).toBe("node-b");

    act(() => result.current.goForwardGraphSelection());
    expect(result.current.selectedGraphEdge?.id).toBe("edge-ab");
  });

  it("drops forward history when selecting after going back", async () => {
    const { result } = renderGraphController();

    await waitFor(() => expect(result.current.selectedGraphNode?.id).toBe("node-a"));

    act(() => result.current.selectGraphNode("node-b"));
    act(() => result.current.selectGraphEdge("edge-ab"));
    act(() => result.current.goBackGraphSelection());
    expect(result.current.selectedGraphNode?.id).toBe("node-b");
    expect(result.current.canGoForwardGraphSelection).toBe(true);

    act(() => result.current.selectGraphNode("node-a"));

    expect(result.current.selectedGraphNode?.id).toBe("node-a");
    expect(result.current.canGoForwardGraphSelection).toBe(false);
  });

  it("prunes invalid selection history entries after graph reload", async () => {
    const { result } = renderGraphController();

    await waitFor(() => expect(result.current.selectedGraphNode?.id).toBe("node-a"));

    act(() => result.current.selectGraphNode("node-b"));
    act(() => result.current.selectGraphEdge("edge-ab"));
    expect(result.current.selectedGraphEdge?.id).toBe("edge-ab");

    mockedGetCodeGraph.mockResolvedValueOnce({
      ...graphResponse,
      stats: { ...graphResponse.stats, returned_nodes: 1, returned_edges: 0 },
      nodes: [graphResponse.nodes[0]],
      edges: [],
    });

    act(() => result.current.refreshGraph());

    await waitFor(() => expect(result.current.selectedGraphNode?.id).toBe("node-a"));
    expect(result.current.selectedGraphEdge).toBeNull();
    expect(result.current.canGoBackGraphSelection).toBe(false);
    expect(result.current.canGoForwardGraphSelection).toBe(false);
  });
});

function renderGraphController() {
  const setStatusMessage = vi.fn();
  const recordLocalDiagnostic = vi.fn();

  return renderHook(() =>
    useGraphController({
      activeTab: "graph",
      project: "memory",
      setStatusMessage,
      recordLocalDiagnostic,
    }),
  );
}

const graphStatus: CodeGraphStatusResponse = {
  project: "memory",
  has_graph: true,
  symbol_count: 2,
  reference_count: 1,
  resolved_reference_count: 1,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 2,
  graph_edge_count: 1,
  evidence_count: 3,
};

const graphResponse: CodeGraphResponse = {
  project: "memory",
  status: graphStatus,
  filters: { depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 2,
    total_edges: 1,
    total_symbols: 2,
    total_references: 1,
    unresolved_references: 0,
    returned_nodes: 2,
    returned_edges: 1,
    seed_nodes: 1,
  },
  truncated: false,
  nodes: [
    {
      id: "node-a",
      stable_identity: "test:node-a",
      label: "NodeA",
      node_kind: "code_symbol",
      language: "typescript",
      symbol_kind: "function",
      file_path: "src/a.ts",
      name: "NodeA",
      qualified_name: "NodeA",
      start_line: 1,
      end_line: 3,
      degree: 1,
      seed: true,
      group: "typescript",
    },
    {
      id: "node-b",
      stable_identity: "test:node-b",
      label: "NodeB",
      node_kind: "code_symbol",
      language: "typescript",
      symbol_kind: "function",
      file_path: "src/b.ts",
      name: "NodeB",
      qualified_name: "NodeB",
      start_line: 5,
      end_line: 8,
      degree: 1,
      seed: false,
      group: "typescript",
    },
  ],
  edges: [
    {
      id: "edge-ab",
      source: "node-a",
      target: "node-b",
      edge_kind: "calls",
      reference_kind: "call",
      confidence: 0.9,
      file_path: "src/a.ts",
      start_line: 2,
      end_line: 2,
      resolution_status: "resolved",
    },
  ],
};

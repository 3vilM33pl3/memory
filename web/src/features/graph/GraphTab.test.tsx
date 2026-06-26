import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { CodeGraphEdge, CodeGraphNode, CodeGraphResponse, CodeGraphStatusResponse } from "../../types";
import { applyConnectedGraphIsolation, GraphTab } from "./GraphTab";

const emptyStatus: CodeGraphStatusResponse = {
  project: "memory",
  has_graph: false,
  symbol_count: 0,
  reference_count: 0,
  resolved_reference_count: 0,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 0,
  graph_edge_count: 0,
  evidence_count: 0,
};

const emptyGraph: CodeGraphResponse = {
  project: "memory",
  status: emptyStatus,
  filters: { depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 0,
    total_edges: 0,
    total_symbols: 0,
    total_references: 0,
    unresolved_references: 0,
    returned_nodes: 0,
    returned_edges: 0,
    seed_nodes: 0,
  },
  truncated: false,
  nodes: [],
  edges: [],
};

const baseProps = {
  project: "memory",
  filters: {
    run_id: "",
    q: "",
    file_path: "",
    symbol: "",
    edge_kind: "",
    depth: 1,
    limit_nodes: 250,
    limit_edges: 500,
    isolate_connected: false,
  },
  loading: false,
  error: null,
  selectedNode: null,
  selectedEdge: null,
  onFilterChange: vi.fn(),
  onSubmit: vi.fn(),
  onRefresh: vi.fn(),
  onSelectNode: vi.fn(),
  onSelectEdge: vi.fn(),
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("GraphTab", () => {
  it("requires WebGL support", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);

    render(<GraphTab {...baseProps} status={emptyStatus} graph={emptyGraph} />);

    expect(await screen.findByRole("heading", { name: "WebGL is required" })).toBeInTheDocument();
  });

  it("shows the extraction command when the project has no graph", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    render(<GraphTab {...baseProps} status={emptyStatus} graph={emptyGraph} />);

    expect(await screen.findByRole("heading", { name: "No code graph extracted" })).toBeInTheDocument();
    expect(screen.getByText("memory graph extract --project memory")).toBeInTheDocument();
  });

  it("emits a local filter change when isolate connected graph is toggled", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);
    const onFilterChange = vi.fn();

    render(<GraphTab {...baseProps} onFilterChange={onFilterChange} status={emptyStatus} graph={emptyGraph} />);

    const checkbox = await screen.findByRole("checkbox", { name: "Isolate connected graph" });
    expect(checkbox).not.toBeChecked();
    fireEvent.click(checkbox);

    expect(onFilterChange).toHaveBeenCalledWith({ isolate_connected: true });
  });
});

describe("applyConnectedGraphIsolation", () => {
  it("leaves the fetched graph unchanged when isolation is disabled", () => {
    expect(applyConnectedGraphIsolation(connectedGraph, false, "node-d")).toBe(connectedGraph);
  });

  it("keeps the whole connected component for the selected node", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, "node-d");

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-d", "node-e"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-de"]);
    expect(isolated?.stats.returned_nodes).toBe(2);
    expect(isolated?.stats.returned_edges).toBe(1);
  });

  it("falls back to the seed component when no selected node is available", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, null);

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b", "node-c"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-ab", "edge-bc"]);
  });
});

const connectedStatus: CodeGraphStatusResponse = {
  project: "memory",
  has_graph: true,
  symbol_count: 5,
  reference_count: 3,
  resolved_reference_count: 3,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 5,
  graph_edge_count: 3,
  evidence_count: 8,
};

const connectedGraph: CodeGraphResponse = {
  project: "memory",
  status: connectedStatus,
  filters: { depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 5,
    total_edges: 3,
    total_symbols: 5,
    total_references: 3,
    unresolved_references: 0,
    returned_nodes: 5,
    returned_edges: 3,
    seed_nodes: 1,
  },
  truncated: false,
  nodes: [
    graphNode("node-a", true),
    graphNode("node-b"),
    graphNode("node-c"),
    graphNode("node-d"),
    graphNode("node-e"),
  ],
  edges: [graphEdge("edge-ab", "node-a", "node-b"), graphEdge("edge-bc", "node-b", "node-c"), graphEdge("edge-de", "node-d", "node-e")],
};

function graphNode(id: string, seed = false): CodeGraphNode {
  return {
    id,
    stable_identity: `test:${id}`,
    label: id,
    node_kind: "code_symbol",
    language: "typescript",
    symbol_kind: "function",
    file_path: "src/example.ts",
    name: id,
    qualified_name: id,
    start_line: 1,
    end_line: 1,
    degree: seed ? 2 : 1,
    seed,
    group: "typescript",
  };
}

function graphEdge(id: string, source: string, target: string): CodeGraphEdge {
  return {
    id,
    source,
    target,
    edge_kind: "calls",
    reference_kind: "call",
    confidence: 0.9,
    file_path: "src/example.ts",
    start_line: 1,
    end_line: 1,
    resolution_status: "resolved",
  };
}

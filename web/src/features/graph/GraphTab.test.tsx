import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { CodeGraphEdge, CodeGraphNode, CodeGraphResponse, CodeGraphStatusResponse } from "../../types";
import { applyConnectedGraphIsolation, buildRenderData, GraphTab } from "./GraphTab";

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
    isolate_depth: 1,
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
  onClearSelection: vi.fn(),
  canGoBackSelection: false,
  canGoForwardSelection: false,
  onGoBackSelection: vi.fn(),
  onGoForwardSelection: vi.fn(),
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

  it("emits a local filter change when isolate degree changes", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);
    const onFilterChange = vi.fn();

    render(
      <GraphTab
        {...baseProps}
        filters={{ ...baseProps.filters, isolate_connected: true }}
        onFilterChange={onFilterChange}
        status={emptyStatus}
        graph={emptyGraph}
      />,
    );

    const degreeInput = await screen.findByRole("spinbutton", { name: "Degrees" });
    expect(degreeInput).toHaveValue(1);
    fireEvent.change(degreeInput, { target: { value: "2" } });

    expect(onFilterChange).toHaveBeenCalledWith({ isolate_depth: 2 });
  });

  it("greys out degree controls until isolation is enabled", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    render(<GraphTab {...baseProps} status={emptyStatus} graph={emptyGraph} />);

    expect(await screen.findByRole("spinbutton", { name: "Degrees" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Decrease graph degrees" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Increase graph degrees" })).toBeDisabled();
  });

  it("emits local filter changes from degree stepper buttons", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);
    const onFilterChange = vi.fn();

    render(
      <GraphTab
        {...baseProps}
        filters={{ ...baseProps.filters, isolate_connected: true }}
        onFilterChange={onFilterChange}
        status={emptyStatus}
        graph={emptyGraph}
      />,
    );

    expect(await screen.findByRole("button", { name: "Decrease graph degrees" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Increase graph degrees" }));
    expect(onFilterChange).toHaveBeenCalledWith({ isolate_depth: 2 });

    cleanup();
    render(
      <GraphTab
        {...baseProps}
        filters={{ ...baseProps.filters, isolate_connected: true, isolate_depth: 2 }}
        onFilterChange={onFilterChange}
        status={emptyStatus}
        graph={emptyGraph}
      />,
    );

    expect(await screen.findByRole("button", { name: "Increase graph degrees" })).not.toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Decrease graph degrees" }));
    expect(onFilterChange).toHaveBeenCalledWith({ isolate_depth: 1 });
  });

  it("renders graph selection history controls with disabled state", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    render(<GraphTab {...baseProps} status={emptyStatus} graph={emptyGraph} />);

    expect(await screen.findByRole("button", { name: "Back" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
  });

  it("calls graph selection history callbacks", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);
    const onGoBackSelection = vi.fn();
    const onGoForwardSelection = vi.fn();

    render(
      <GraphTab
        {...baseProps}
        canGoBackSelection
        canGoForwardSelection
        onGoBackSelection={onGoBackSelection}
        onGoForwardSelection={onGoForwardSelection}
        status={emptyStatus}
        graph={emptyGraph}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Back" }));
    fireEvent.click(screen.getByRole("button", { name: "Forward" }));

    expect(onGoBackSelection).toHaveBeenCalled();
    expect(onGoForwardSelection).toHaveBeenCalled();
  });
});

describe("applyConnectedGraphIsolation", () => {
  it("leaves the fetched graph unchanged when isolation is disabled", () => {
    expect(applyConnectedGraphIsolation(connectedGraph, false, "node-d", 1)).toBe(connectedGraph);
  });

  it("keeps direct neighbors for a one-degree selected-node radius", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, "node-a", 1);

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-ab"]);
    expect(isolated?.stats.returned_nodes).toBe(2);
    expect(isolated?.stats.returned_edges).toBe(1);
  });

  it("includes second-hop neighbors for a two-degree selected-node radius", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, "node-a", 2);

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b", "node-c"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-ab", "edge-bc"]);
    expect(isolated?.stats.returned_nodes).toBe(3);
    expect(isolated?.stats.returned_edges).toBe(2);
  });

  it("includes third-hop neighbors when the isolate radius is raised above two", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, "node-a", 3);

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b", "node-c", "node-f"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-ab", "edge-bc", "edge-cf"]);
    expect(isolated?.nodes.map((node) => visibleDegree(node))).toEqual([0, 1, 2, 3]);
    expect(isolated?.stats.returned_nodes).toBe(4);
    expect(isolated?.stats.returned_edges).toBe(3);
  });

  it("falls back to the seed component when no selected node is available", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, null, 2);

    expect(isolated?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b", "node-c"]);
    expect(isolated?.edges.map((edge) => edge.id)).toEqual(["edge-ab", "edge-bc"]);
  });
});

describe("buildRenderData", () => {
  it("makes the selected node high contrast and larger than normal nodes", () => {
    const isolated = applyConnectedGraphIsolation(connectedGraph, true, "node-a", 2);

    const renderData = buildRenderData(isolated, "node-a", null);
    const selectedNode = renderData.nodes.find((node) => node.id === "node-a");
    const neighborNode = renderData.nodes.find((node) => node.id === "node-b");

    expect(selectedNode?.selected).toBe(true);
    expect(selectedNode?.color).toBe("#ffffff");
    expect(neighborNode?.color).not.toBe("#ffffff");
    expect(selectedNode?.val ?? 0).toBeGreaterThan(neighborNode?.val ?? 0);
  });
});

const connectedStatus: CodeGraphStatusResponse = {
  project: "memory",
  has_graph: true,
  symbol_count: 6,
  reference_count: 4,
  resolved_reference_count: 4,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 6,
  graph_edge_count: 4,
  evidence_count: 8,
};

const connectedGraph: CodeGraphResponse = {
  project: "memory",
  status: connectedStatus,
  filters: { depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 6,
    total_edges: 4,
    total_symbols: 6,
    total_references: 4,
    unresolved_references: 0,
    returned_nodes: 6,
    returned_edges: 4,
    seed_nodes: 1,
  },
  truncated: false,
  nodes: [
    graphNode("node-a", true),
    graphNode("node-b"),
    graphNode("node-c"),
    graphNode("node-f"),
    graphNode("node-d"),
    graphNode("node-e"),
  ],
  edges: [
    graphEdge("edge-ab", "node-a", "node-b"),
    graphEdge("edge-bc", "node-b", "node-c"),
    graphEdge("edge-cf", "node-c", "node-f"),
    graphEdge("edge-de", "node-d", "node-e"),
  ],
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

function visibleDegree(node: CodeGraphNode): number | undefined {
  return (node as CodeGraphNode & { isolate_degree?: number }).isolate_degree;
}

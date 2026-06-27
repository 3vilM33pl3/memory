import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const forceGraphMock = vi.hoisted(() => ({
  instances: [] as Array<{
    backgroundColor: ReturnType<typeof vi.fn>;
    showNavInfo: ReturnType<typeof vi.fn>;
    nodeId: ReturnType<typeof vi.fn>;
    linkSource: ReturnType<typeof vi.fn>;
    linkTarget: ReturnType<typeof vi.fn>;
    nodeLabel: ReturnType<typeof vi.fn>;
    linkLabel: ReturnType<typeof vi.fn>;
    nodeVal: ReturnType<typeof vi.fn>;
    nodeColor: ReturnType<typeof vi.fn>;
    linkColor: ReturnType<typeof vi.fn>;
    linkWidth: ReturnType<typeof vi.fn>;
    linkOpacity: ReturnType<typeof vi.fn>;
    linkDirectionalArrowLength: ReturnType<typeof vi.fn>;
    linkDirectionalArrowRelPos: ReturnType<typeof vi.fn>;
    onNodeClick: ReturnType<typeof vi.fn>;
    onLinkClick: ReturnType<typeof vi.fn>;
    onNodeHover: ReturnType<typeof vi.fn>;
    onLinkHover: ReturnType<typeof vi.fn>;
    onBackgroundClick: ReturnType<typeof vi.fn>;
    width: ReturnType<typeof vi.fn>;
    height: ReturnType<typeof vi.fn>;
    graphData: ReturnType<typeof vi.fn>;
    zoomToFit: ReturnType<typeof vi.fn>;
    _destructor: ReturnType<typeof vi.fn>;
  }>,
}));

vi.mock("3d-force-graph", () => {
  const createInstance = () => {
    const instance = {
      backgroundColor: vi.fn(() => instance),
      showNavInfo: vi.fn(() => instance),
      nodeId: vi.fn(() => instance),
      linkSource: vi.fn(() => instance),
      linkTarget: vi.fn(() => instance),
      nodeLabel: vi.fn(() => instance),
      linkLabel: vi.fn(() => instance),
      nodeVal: vi.fn(() => instance),
      nodeColor: vi.fn(() => instance),
      linkColor: vi.fn(() => instance),
      linkWidth: vi.fn(() => instance),
      linkOpacity: vi.fn(() => instance),
      linkDirectionalArrowLength: vi.fn(() => instance),
      linkDirectionalArrowRelPos: vi.fn(() => instance),
      onNodeClick: vi.fn(() => instance),
      onLinkClick: vi.fn(() => instance),
      onNodeHover: vi.fn(() => instance),
      onLinkHover: vi.fn(() => instance),
      onBackgroundClick: vi.fn(() => instance),
      width: vi.fn(() => instance),
      height: vi.fn(() => instance),
      graphData: vi.fn(() => instance),
      zoomToFit: vi.fn(() => instance),
      _destructor: vi.fn(),
    };
    forceGraphMock.instances.push(instance);
    return instance;
  };
  function MockForceGraph3D() {
    return createInstance();
  }
  return { default: MockForceGraph3D };
});

import type {
  CodeGraphEdge,
  CodeGraphNode,
  CodeGraphResponse,
  CodeGraphStatusResponse,
  ProjectMemoryGraphResponse,
} from "../../types";
import {
  applyConnectedGraphIsolation,
  applyGraphConnectionView,
  buildLayeredRenderData,
  buildRenderData,
  graphRenderTopologySignature,
  GraphTab,
} from "./GraphTab";

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
  memoryGraph: null,
  selectedNode: null,
  selectedEdge: null,
  connectionView: null,
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
  vi.useRealTimers();
  forceGraphMock.instances = [];
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

  it("renders layer controls below the graph with code enabled by default", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    render(
      <GraphTab
        {...baseProps}
        status={connectedStatus}
        graph={connectedGraph}
        memoryGraph={memoryGraph}
      />,
    );

    expect(await screen.findByRole("checkbox", { name: /Code/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /Provenance/ })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: /Memory relationships/ })).not.toBeChecked();
    expect(screen.getByText("2 edges")).toBeInTheDocument();
    expect(screen.getByText("1 edges")).toBeInTheDocument();
  });

  it("toggles memory graph layers independently from code filters", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    render(
      <GraphTab
        {...baseProps}
        status={connectedStatus}
        graph={connectedGraph}
        memoryGraph={memoryGraph}
      />,
    );

    const provenance = await screen.findByRole("checkbox", { name: /Provenance/ });
    fireEvent.click(provenance);

    expect(provenance).toBeChecked();
    expect(baseProps.onFilterChange).not.toHaveBeenCalled();
  });

  it("shows connection summary text when a connection view is active", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);

    render(
      <GraphTab
        {...baseProps}
        status={connectionStatus}
        graph={{ ...connectionGraph, status: { ...connectionStatus, has_graph: false } }}
        selectedNode={connectionGraph.nodes.find((node) => node.id === "node-d") ?? null}
        connectionView={{ sourceNodeId: "node-a", targetNodeId: "node-d" }}
      />,
    );

    expect(await screen.findByText("connecting node-a to node-d, showing 4 / 4")).toBeInTheDocument();
  });

  it("does not refit the 3d graph when only selected node styling changes", async () => {
    vi.useFakeTimers();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({} as RenderingContext);

    const { rerender } = render(
      <GraphTab {...baseProps} status={connectedStatus} graph={connectedGraph} selectedNode={connectedGraph.nodes[0]} />,
    );

    expect(screen.getByTestId("graph-scene")).toBeInTheDocument();
    expect(forceGraphMock.instances).toHaveLength(1);
    act(() => {
      vi.runOnlyPendingTimers();
    });
    expect(forceGraphMock.instances[0].zoomToFit).toHaveBeenCalledTimes(1);

    rerender(
      <GraphTab {...baseProps} status={connectedStatus} graph={connectedGraph} selectedNode={connectedGraph.nodes[1]} />,
    );
    act(() => {
      vi.runOnlyPendingTimers();
    });

    expect(forceGraphMock.instances[0].graphData).toHaveBeenCalledTimes(2);
    expect(forceGraphMock.instances[0].zoomToFit).toHaveBeenCalledTimes(1);
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

describe("applyGraphConnectionView", () => {
  it("includes all parallel connecting paths instead of only the shortest path", () => {
    const connected = applyGraphConnectionView(connectionGraph, { sourceNodeId: "node-a", targetNodeId: "node-d" });

    expect(connected?.nodes.map((node) => node.id)).toEqual(["node-a", "node-b", "node-c", "node-d"]);
    expect(connected?.edges.map((edge) => edge.id)).toEqual(["edge-ab", "edge-bd", "edge-ac", "edge-cd"]);
    expect(connected?.stats.returned_nodes).toBe(4);
    expect(connected?.stats.returned_edges).toBe(4);
  });

  it("excludes side branches and side cycles that do not connect both endpoints", () => {
    const connected = applyGraphConnectionView(connectionGraph, { sourceNodeId: "node-a", targetNodeId: "node-d" });

    expect(connected?.nodes.map((node) => node.id)).not.toContain("node-e");
    expect(connected?.nodes.map((node) => node.id)).not.toContain("node-g");
    expect(connected?.edges.map((edge) => edge.id)).not.toContain("edge-be");
    expect(connected?.edges.map((edge) => edge.id)).not.toContain("edge-eg");
    expect(connected?.edges.map((edge) => edge.id)).not.toContain("edge-gb");
  });

  it("returns only endpoints when the endpoints are disconnected", () => {
    const connected = applyGraphConnectionView(connectionGraph, { sourceNodeId: "node-a", targetNodeId: "node-f" });

    expect(connected?.nodes.map((node) => node.id)).toEqual(["node-a", "node-f"]);
    expect(connected?.edges).toEqual([]);
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

describe("graphRenderTopologySignature", () => {
  it("ignores styling-only selection changes but changes when topology changes", () => {
    const unselected = buildRenderData(connectedGraph, null, null);
    const selected = buildRenderData(connectedGraph, "node-a", null);
    const isolated = buildRenderData(applyConnectedGraphIsolation(connectedGraph, true, "node-a", 1), "node-a", null);

    expect(graphRenderTopologySignature(selected)).toBe(graphRenderTopologySignature(unselected));
    expect(graphRenderTopologySignature(isolated)).not.toBe(graphRenderTopologySignature(unselected));
  });
});

describe("buildLayeredRenderData", () => {
  it("keeps memory graph layers hidden until toggled on", () => {
    const renderData = buildLayeredRenderData({
      codeGraph: connectedGraph,
      memoryGraph,
      visibleLayers: { code: true, provenance: false, memory_relations: false },
      hoveredLayer: null,
      selectedNodeId: null,
      selectedEdgeId: null,
      memorySelection: null,
    });

    expect(renderData.nodes.some((node) => node.renderKind === "memory_node")).toBe(false);
    expect(renderData.links.some((link) => link.renderKind === "provenance_edge")).toBe(false);
  });

  it("adds provenance nodes and dims other layers when provenance is hovered", () => {
    const renderData = buildLayeredRenderData({
      codeGraph: connectedGraph,
      memoryGraph,
      visibleLayers: { code: true, provenance: true, memory_relations: false },
      hoveredLayer: "provenance",
      selectedNodeId: null,
      selectedEdgeId: null,
      memorySelection: null,
    });

    expect(renderData.nodes.some((node) => node.renderKind === "memory_node")).toBe(true);
    expect(renderData.nodes.some((node) => node.renderKind === "source_node")).toBe(true);
    expect(renderData.links.filter((link) => link.renderKind === "provenance_edge")).toHaveLength(2);
    expect(renderData.nodes.find((node) => node.primaryLayer === "code")?.color).toBe("#2d3744");
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

const connectionStatus: CodeGraphStatusResponse = {
  project: "memory",
  has_graph: true,
  symbol_count: 7,
  reference_count: 7,
  resolved_reference_count: 7,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 7,
  graph_edge_count: 7,
  evidence_count: 14,
};

const connectionGraph: CodeGraphResponse = {
  project: "memory",
  status: connectionStatus,
  filters: { depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 7,
    total_edges: 7,
    total_symbols: 7,
    total_references: 7,
    unresolved_references: 0,
    returned_nodes: 7,
    returned_edges: 7,
    seed_nodes: 1,
  },
  truncated: false,
  nodes: [
    graphNode("node-a", true),
    graphNode("node-b"),
    graphNode("node-c"),
    graphNode("node-d"),
    graphNode("node-e"),
    graphNode("node-f"),
    graphNode("node-g"),
  ],
  edges: [
    graphEdge("edge-ab", "node-a", "node-b"),
    graphEdge("edge-bd", "node-b", "node-d"),
    graphEdge("edge-ac", "node-a", "node-c"),
    graphEdge("edge-cd", "node-c", "node-d"),
    graphEdge("edge-be", "node-b", "node-e"),
    graphEdge("edge-eg", "node-e", "node-g"),
    graphEdge("edge-gb", "node-g", "node-b"),
  ],
};

const memoryGraph: ProjectMemoryGraphResponse = {
  project: "memory",
  total_memories: 2,
  returned_memories: 2,
  nodes: [
    {
      id: "memory:11111111-1111-4111-8111-111111111111",
      label: "Graph endpoint exposes provenance",
      node_kind: "memory",
      memory_id: "11111111-1111-4111-8111-111111111111",
      memory_type: "implementation",
      confidence: 0.91,
      importance: 4,
      tags: ["graph"],
      summary: "Graph endpoint exposes provenance",
    },
    {
      id: "memory:22222222-2222-4222-8222-222222222222",
      label: "Graph endpoint exposes relations",
      node_kind: "memory",
      memory_id: "22222222-2222-4222-8222-222222222222",
      memory_type: "architecture",
      confidence: 0.88,
      importance: 3,
      tags: ["graph"],
      summary: "Graph endpoint exposes relations",
    },
    {
      id: "source:file:src/graph.rs::build_memory_graph",
      label: "src/graph.rs::build_memory_graph",
      node_kind: "source",
      source_id: "33333333-3333-4333-8333-333333333333",
      source_kind: "file",
      tags: [],
      file_path: "src/graph.rs",
      symbol_name: "build_memory_graph",
      symbol_kind: "function",
      provenance_status: "verified",
    },
  ],
  edges: [
    {
      id: "provenance:11111111-1111-4111-8111-111111111111:source:file:src/graph.rs::build_memory_graph",
      source: "memory:11111111-1111-4111-8111-111111111111",
      target: "source:file:src/graph.rs::build_memory_graph",
      edge_kind: "provenance",
      source_kind: "file",
    },
    {
      id: "provenance:22222222-2222-4222-8222-222222222222:source:file:src/graph.rs::build_memory_graph",
      source: "memory:22222222-2222-4222-8222-222222222222",
      target: "source:file:src/graph.rs::build_memory_graph",
      edge_kind: "provenance",
      source_kind: "file",
    },
    {
      id: "relation:11111111-1111-4111-8111-111111111111:supports:22222222-2222-4222-8222-222222222222",
      source: "memory:11111111-1111-4111-8111-111111111111",
      target: "memory:22222222-2222-4222-8222-222222222222",
      edge_kind: "memory_relation",
      relation_type: "supports",
    },
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

import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { CodeGraphResponse, CodeGraphStatusResponse } from "../../types";
import { GraphTab } from "./GraphTab";

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
});

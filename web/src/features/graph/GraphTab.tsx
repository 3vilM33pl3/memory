import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import ForceGraph3D, { type ForceGraph3DInstance } from "3d-force-graph";

import type { CodeGraphEdge, CodeGraphNode, CodeGraphResponse, CodeGraphStatusResponse } from "../../types";
import type { GraphFilterForm } from "./useGraphController";

type RenderNode = CodeGraphNode & {
  val: number;
  color: string;
};

type RenderLink = CodeGraphEdge & {
  color: string;
  width: number;
};

const MAX_ISOLATE_DEPTH = 2;

interface GraphTabProps {
  project: string;
  filters: GraphFilterForm;
  status: CodeGraphStatusResponse | null;
  graph: CodeGraphResponse | null;
  loading: boolean;
  error: string | null;
  selectedNode: CodeGraphNode | null;
  selectedEdge: CodeGraphEdge | null;
  onFilterChange: (patch: Partial<GraphFilterForm>) => void;
  onSubmit: (event: FormEvent) => void;
  onRefresh: () => void;
  onSelectNode: (nodeId: string | null) => void;
  onSelectEdge: (edgeId: string | null) => void;
}

export function GraphTab({
  project,
  filters,
  status,
  graph,
  loading,
  error,
  selectedNode,
  selectedEdge,
  onFilterChange,
  onSubmit,
  onRefresh,
  onSelectNode,
  onSelectEdge,
}: GraphTabProps) {
  const [webglSupported, setWebglSupported] = useState(true);
  const visibleGraph = useMemo(
    () => applyConnectedGraphIsolation(graph, filters.isolate_connected, selectedNode?.id ?? null, filters.isolate_depth),
    [filters.isolate_connected, filters.isolate_depth, graph, selectedNode?.id],
  );
  const visibleSelectedNode = useMemo(
    () => (selectedNode && visibleGraph?.nodes.some((node) => node.id === selectedNode.id) ? selectedNode : null),
    [selectedNode, visibleGraph?.nodes],
  );
  const visibleSelectedEdge = useMemo(
    () => (selectedEdge && visibleGraph?.edges.some((edge) => edge.id === selectedEdge.id) ? selectedEdge : null),
    [selectedEdge, visibleGraph?.edges],
  );

  useEffect(() => {
    setWebglSupported(hasWebGLSupport());
  }, []);

  return (
    <section className="graph-page">
      <form className="graph-toolbar" onSubmit={onSubmit}>
        <label>
          Search
          <input value={filters.q} onChange={(event) => onFilterChange({ q: event.target.value })} />
        </label>
        <label>
          File
          <input value={filters.file_path} onChange={(event) => onFilterChange({ file_path: event.target.value })} />
        </label>
        <label>
          Symbol
          <input value={filters.symbol} onChange={(event) => onFilterChange({ symbol: event.target.value })} />
        </label>
        <label>
          Edge
          <input value={filters.edge_kind} onChange={(event) => onFilterChange({ edge_kind: event.target.value })} />
        </label>
        <label>
          Depth
          <select value={filters.depth} onChange={(event) => onFilterChange({ depth: Number(event.target.value) })}>
            <option value={0}>0</option>
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </label>
        <label>
          Nodes
          <input
            min={1}
            max={1000}
            type="number"
            value={filters.limit_nodes}
            onChange={(event) => onFilterChange({ limit_nodes: Number(event.target.value) })}
          />
        </label>
        <label>
          Edges
          <input
            min={1}
            max={2000}
            type="number"
            value={filters.limit_edges}
            onChange={(event) => onFilterChange({ limit_edges: Number(event.target.value) })}
          />
        </label>
        <label className="graph-checkbox">
          <input
            type="checkbox"
            checked={filters.isolate_connected}
            onChange={(event) => onFilterChange({ isolate_connected: event.target.checked })}
          />
          Isolate connected graph
        </label>
        <div className="graph-degree">
          <label htmlFor="graph-isolate-depth">Degrees</label>
          <span className="graph-degree-stepper">
            <button
              type="button"
              aria-label="Decrease graph degrees"
              disabled={!filters.isolate_connected || normalizeIsolationDepth(filters.isolate_depth) <= 1}
              onClick={() => onFilterChange({ isolate_depth: normalizeIsolationDepth(filters.isolate_depth) - 1 })}
            >
              -
            </button>
            <input
              id="graph-isolate-depth"
              min={1}
              max={MAX_ISOLATE_DEPTH}
              step={1}
              type="number"
              value={filters.isolate_depth}
              disabled={!filters.isolate_connected}
              onChange={(event) => onFilterChange({ isolate_depth: Number(event.target.value) })}
            />
            <button
              type="button"
              aria-label="Increase graph degrees"
              disabled={!filters.isolate_connected || normalizeIsolationDepth(filters.isolate_depth) >= MAX_ISOLATE_DEPTH}
              onClick={() => onFilterChange({ isolate_depth: normalizeIsolationDepth(filters.isolate_depth) + 1 })}
            >
              +
            </button>
          </span>
        </div>
        <button type="submit" disabled={loading}>{loading ? "Loading..." : "Apply"}</button>
        <button type="button" onClick={onRefresh} disabled={loading}>Refresh</button>
      </form>

      <div className="graph-summary">
        <span>{project}</span>
        <span>{status?.has_graph ? `${status.graph_node_count} nodes / ${status.graph_edge_count} edges` : "no extracted graph"}</span>
        {graph ? (
          <GraphShowingSummary
            graph={graph}
            visibleGraph={visibleGraph}
            isolateConnected={filters.isolate_connected}
            isolateDepth={filters.isolate_depth}
          />
        ) : null}
        {graph?.truncated ? <span className="warning-inline">{graph.truncation_reason}</span> : null}
        {error ? <span className="warning-inline">{error}</span> : null}
      </div>

      {!webglSupported ? (
        <div className="graph-empty">
          <h2>WebGL is required</h2>
          <p>This graph explorer needs a browser with WebGL enabled.</p>
        </div>
      ) : graph && !graph.status.has_graph ? (
        <div className="graph-empty">
          <h2>No code graph extracted</h2>
          <p>Run <code>memory graph extract --project {project}</code> and refresh this tab.</p>
        </div>
      ) : (
        <div className="graph-workspace">
          <GraphScene
            graph={visibleGraph}
            selectedNode={visibleSelectedNode}
            selectedEdge={visibleSelectedEdge}
            onSelectNode={onSelectNode}
            onSelectEdge={onSelectEdge}
          />
          <GraphInspector node={visibleSelectedNode} edge={visibleSelectedEdge} graph={visibleGraph} />
        </div>
      )}
    </section>
  );
}

function GraphShowingSummary({
  graph,
  visibleGraph,
  isolateConnected,
  isolateDepth,
}: {
  graph: CodeGraphResponse;
  visibleGraph: CodeGraphResponse | null;
  isolateConnected: boolean;
  isolateDepth: number;
}) {
  if (!isolateConnected) {
    return <span>showing {graph.stats.returned_nodes} / {graph.stats.returned_edges}</span>;
  }
  const normalizedDepth = normalizeIsolationDepth(isolateDepth);
  const degreeLabel = normalizedDepth === 1 ? "degree" : "degrees";
  return (
    <span>
      showing {visibleGraph?.stats.returned_nodes ?? 0} / {visibleGraph?.stats.returned_edges ?? 0} within{" "}
      {normalizedDepth} {degreeLabel} from{" "}
      {graph.stats.returned_nodes} / {graph.stats.returned_edges}
    </span>
  );
}

function GraphScene({
  graph,
  selectedNode,
  selectedEdge,
  onSelectNode,
  onSelectEdge,
}: {
  graph: CodeGraphResponse | null;
  selectedNode: CodeGraphNode | null;
  selectedEdge: CodeGraphEdge | null;
  onSelectNode: (nodeId: string | null) => void;
  onSelectEdge: (edgeId: string | null) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const instanceRef = useRef<ForceGraph3DInstance<RenderNode, RenderLink> | null>(null);
  const onSelectNodeRef = useRef(onSelectNode);
  const onSelectEdgeRef = useRef(onSelectEdge);
  const renderData = useMemo(() => buildRenderData(graph, selectedNode?.id ?? null, selectedEdge?.id ?? null), [graph, selectedEdge?.id, selectedNode?.id]);

  useEffect(() => {
    onSelectNodeRef.current = onSelectNode;
    onSelectEdgeRef.current = onSelectEdge;
  }, [onSelectEdge, onSelectNode]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const instance = new ForceGraph3D(container, {
      controlType: "orbit",
      rendererConfig: { antialias: true, alpha: false },
    }) as unknown as ForceGraph3DInstance<RenderNode, RenderLink>;
    instance
      .backgroundColor("#081019")
      .showNavInfo(false)
      .nodeId("id")
      .linkSource("source")
      .linkTarget("target")
      .nodeLabel((node) => nodeLabel(node))
      .linkLabel((link) => linkLabel(link))
      .nodeVal((node) => node.val)
      .nodeColor((node) => node.color)
      .linkColor((link) => link.color)
      .linkWidth((link) => link.width)
      .linkOpacity(0.45)
      .linkDirectionalArrowLength(3)
      .linkDirectionalArrowRelPos(1)
      .onNodeClick((node) => onSelectNodeRef.current(node.id))
      .onLinkClick((link) => onSelectEdgeRef.current(link.id))
      .onBackgroundClick(() => {
        onSelectNodeRef.current(null);
        onSelectEdgeRef.current(null);
      });
    instanceRef.current = instance;

    const resize = () => {
      instance.width(container.clientWidth || 960).height(container.clientHeight || 640);
    };
    resize();
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(resize) : null;
    observer?.observe(container);

    return () => {
      observer?.disconnect();
      instance._destructor();
      instanceRef.current = null;
      container.replaceChildren();
    };
  }, []);

  useEffect(() => {
    const instance = instanceRef.current;
    if (!instance) return;
    instance.graphData(renderData);
    if (renderData.nodes.length) {
      window.setTimeout(() => instance.zoomToFit(500, 48), 50);
    }
  }, [renderData]);

  return <div ref={containerRef} className="graph-scene" data-testid="graph-scene" />;
}

function GraphInspector({
  node,
  edge,
  graph,
}: {
  node: CodeGraphNode | null;
  edge: CodeGraphEdge | null;
  graph: CodeGraphResponse | null;
}) {
  if (node) {
    return (
      <aside className="graph-inspector">
        <h2>{node.label}</h2>
        <dl>
          <dt>Kind</dt><dd>{node.symbol_kind ?? node.node_kind}</dd>
          <dt>Group</dt><dd>{node.group}</dd>
          <dt>File</dt><dd>{node.file_path ?? "n/a"}</dd>
          <dt>Lines</dt><dd>{lineRange(node.start_line, node.end_line)}</dd>
          <dt>Degree</dt><dd>{node.degree}</dd>
          <dt>Identity</dt><dd><code>{node.stable_identity}</code></dd>
        </dl>
      </aside>
    );
  }
  if (edge) {
    const source = graph?.nodes.find((candidate) => candidate.id === edge.source);
    const target = graph?.nodes.find((candidate) => candidate.id === edge.target);
    return (
      <aside className="graph-inspector">
        <h2>{edge.edge_kind}</h2>
        <dl>
          <dt>Source</dt><dd>{source?.label ?? edge.source}</dd>
          <dt>Target</dt><dd>{target?.label ?? edge.target}</dd>
          <dt>Reference</dt><dd>{edge.reference_kind ?? "n/a"}</dd>
          <dt>File</dt><dd>{edge.file_path ?? "n/a"}</dd>
          <dt>Lines</dt><dd>{lineRange(edge.start_line, edge.end_line)}</dd>
          <dt>Confidence</dt><dd>{edge.confidence.toFixed(2)}</dd>
        </dl>
      </aside>
    );
  }
  return (
    <aside className="graph-inspector">
      <h2>Selection</h2>
      <p className="muted">No node or edge selected.</p>
    </aside>
  );
}

export function hasWebGLSupport(): boolean {
  const canvas = document.createElement("canvas");
  return Boolean(canvas.getContext("webgl2") ?? canvas.getContext("webgl"));
}

export function applyConnectedGraphIsolation(
  graph: CodeGraphResponse | null,
  isolateConnected: boolean,
  selectedNodeId: string | null,
  isolateDepth: number,
): CodeGraphResponse | null {
  if (!graph || !isolateConnected) return graph;

  const graphNodeIds = new Set(graph.nodes.map((node) => node.id));
  const anchorNodeId = resolveConnectedGraphAnchor(graph, graphNodeIds, selectedNodeId);
  if (!anchorNodeId) {
    return {
      ...graph,
      stats: { ...graph.stats, returned_nodes: 0, returned_edges: 0, seed_nodes: 0 },
      nodes: [],
      edges: [],
    };
  }
  const maxDepth = normalizeIsolationDepth(isolateDepth);

  const adjacency = new Map<string, Set<string>>();
  for (const nodeId of graphNodeIds) adjacency.set(nodeId, new Set());
  for (const edge of graph.edges) {
    if (!graphNodeIds.has(edge.source) || !graphNodeIds.has(edge.target)) continue;
    adjacency.get(edge.source)?.add(edge.target);
    adjacency.get(edge.target)?.add(edge.source);
  }

  const visibleNodeIds = new Set<string>();
  const pending = [{ nodeId: anchorNodeId, depth: 0 }];
  while (pending.length) {
    const next = pending.pop();
    const nodeId = next?.nodeId;
    if (!nodeId || visibleNodeIds.has(nodeId)) continue;
    visibleNodeIds.add(nodeId);
    if ((next?.depth ?? 0) >= maxDepth) continue;
    for (const nextNodeId of adjacency.get(nodeId) ?? []) {
      if (!visibleNodeIds.has(nextNodeId)) pending.push({ nodeId: nextNodeId, depth: (next?.depth ?? 0) + 1 });
    }
  }

  const nodes = graph.nodes.filter((node) => visibleNodeIds.has(node.id));
  const edges = graph.edges.filter((edge) => visibleNodeIds.has(edge.source) && visibleNodeIds.has(edge.target));
  return {
    ...graph,
    stats: {
      ...graph.stats,
      returned_nodes: nodes.length,
      returned_edges: edges.length,
      seed_nodes: nodes.filter((node) => node.seed).length,
    },
    nodes,
    edges,
  };
}

function resolveConnectedGraphAnchor(
  graph: CodeGraphResponse,
  graphNodeIds: Set<string>,
  selectedNodeId: string | null,
): string | null {
  if (selectedNodeId && graphNodeIds.has(selectedNodeId)) return selectedNodeId;
  return graph.nodes.find((node) => node.seed)?.id ?? graph.nodes[0]?.id ?? null;
}

function normalizeIsolationDepth(isolateDepth: number): number {
  if (!Number.isFinite(isolateDepth)) return 1;
  return Math.max(1, Math.min(MAX_ISOLATE_DEPTH, Math.floor(isolateDepth)));
}

function buildRenderData(
  graph: CodeGraphResponse | null,
  selectedNodeId: string | null,
  selectedEdgeId: string | null,
): { nodes: RenderNode[]; links: RenderLink[] } {
  const nodes = (graph?.nodes ?? []).map((node) => ({
    ...node,
    val: Math.max(3, Math.min(14, 3 + node.degree)),
    color: node.id === selectedNodeId ? "#ffc96b" : node.seed ? "#7be0c5" : colorForGroup(node.group),
  }));
  const links = (graph?.edges ?? []).map((edge) => ({
    ...edge,
    color: edge.id === selectedEdgeId ? "#ffc96b" : colorForEdge(edge.edge_kind),
    width: edge.id === selectedEdgeId ? 2.8 : Math.max(0.8, Math.min(2.2, edge.confidence * 2)),
  }));
  return { nodes, links };
}

function nodeLabel(node: RenderNode): string {
  const location = node.file_path ? `${node.file_path}:${node.start_line ?? "?"}` : "no file";
  return `${node.label}<br/>${node.symbol_kind ?? node.node_kind}<br/>${location}`;
}

function linkLabel(link: RenderLink): string {
  return `${link.edge_kind}<br/>${link.file_path ?? "no file"}:${link.start_line ?? "?"}`;
}

function colorForGroup(group: string): string {
  const palette = ["#8be3a0", "#8ab4ff", "#d7a8ff", "#ffb082", "#f48fb1", "#9ad7d1"];
  let hash = 0;
  for (const char of group) hash = (hash * 31 + char.charCodeAt(0)) % 997;
  return palette[hash % palette.length];
}

function colorForEdge(edgeKind: string): string {
  if (edgeKind.includes("call")) return "#7be0c5";
  if (edgeKind.includes("test")) return "#ffc96b";
  if (edgeKind.includes("import")) return "#8ab4ff";
  return "#9cb0c6";
}

function lineRange(start?: number | null, end?: number | null): string {
  if (!start && !end) return "n/a";
  if (start === end || !end) return String(start);
  return `${start}-${end}`;
}

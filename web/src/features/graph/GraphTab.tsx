import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import ForceGraph3D, { type ForceGraph3DInstance } from "3d-force-graph";

import type {
  CodeGraphEdge,
  CodeGraphNode,
  CodeGraphResponse,
  CodeGraphStatusResponse,
  ProjectMemoryGraphEdge,
  ProjectMemoryGraphNode,
  ProjectMemoryGraphResponse,
} from "../../types";
import type { GraphConnectionView, GraphFilterForm } from "./useGraphController";

type GraphLayer = "code" | "provenance" | "memory_relations";

interface LayerVisibility {
  code: boolean;
  provenance: boolean;
  memory_relations: boolean;
}

type RenderNode = Partial<CodeGraphNode> & {
  id: string;
  label: string;
  val: number;
  color: string;
  selected: boolean;
  layers: GraphLayer[];
  primaryLayer: GraphLayer;
  renderKind: "code_node" | "memory_node" | "source_node";
  memoryNode?: ProjectMemoryGraphNode;
  isolate_degree?: number;
};

type RenderLink = Partial<CodeGraphEdge> & {
  id: string;
  source: string;
  target: string;
  color: string;
  width: number;
  layers: GraphLayer[];
  primaryLayer: GraphLayer;
  renderKind: "code_edge" | "provenance_edge" | "memory_attachment_edge" | "memory_relation_edge";
  memoryEdge?: ProjectMemoryGraphEdge;
};

const MAX_ISOLATE_DEPTH = 8;
const MAX_MEMORY_DEPTH = 8;

const DEFAULT_LAYER_VISIBILITY: LayerVisibility = {
  code: true,
  provenance: false,
  memory_relations: false,
};

type MemoryGraphSelection =
  | { kind: "memory_node"; node: ProjectMemoryGraphNode }
  | { kind: "source_node"; node: ProjectMemoryGraphNode }
  | { kind: "provenance_edge"; edge: ProjectMemoryGraphEdge }
  | { kind: "memory_attachment_edge"; edge: ProjectMemoryGraphEdge }
  | { kind: "memory_relation_edge"; edge: ProjectMemoryGraphEdge };

interface GraphTabProps {
  project: string;
  filters: GraphFilterForm;
  status: CodeGraphStatusResponse | null;
  graph: CodeGraphResponse | null;
  memoryGraph: ProjectMemoryGraphResponse | null;
  loading: boolean;
  error: string | null;
  selectedNode: CodeGraphNode | null;
  selectedEdge: CodeGraphEdge | null;
  connectionView: GraphConnectionView | null;
  onFilterChange: (patch: Partial<GraphFilterForm>) => void;
  onSubmit: (event: FormEvent) => void;
  onRefresh: () => void;
  onSelectNode: (nodeId: string | null, options?: { shiftKey?: boolean }) => void;
  onSelectEdge: (edgeId: string | null) => void;
  onClearSelection: () => void;
  canGoBackSelection: boolean;
  canGoForwardSelection: boolean;
  onGoBackSelection: () => void;
  onGoForwardSelection: () => void;
}

export function GraphTab({
  project,
  filters,
  status,
  graph,
  memoryGraph,
  loading,
  error,
  selectedNode,
  selectedEdge,
  connectionView,
  onFilterChange,
  onSubmit,
  onRefresh,
  onSelectNode,
  onSelectEdge,
  onClearSelection,
  canGoBackSelection,
  canGoForwardSelection,
  onGoBackSelection,
  onGoForwardSelection,
}: GraphTabProps) {
  const [webglSupported, setWebglSupported] = useState(true);
  const [visibleLayers, setVisibleLayers] = useState<LayerVisibility>(DEFAULT_LAYER_VISIBILITY);
  const [hoveredLayer, setHoveredLayer] = useState<GraphLayer | null>(null);
  const [memorySelection, setMemorySelection] = useState<MemoryGraphSelection | null>(null);
  const [memoryDepth, setMemoryDepth] = useState(1);
  const connectionGraph = useMemo(
    () => applyGraphConnectionView(graph, connectionView),
    [connectionView, graph],
  );
  const visibleGraph = useMemo(
    () =>
      connectionGraph ??
      applyConnectedGraphIsolation(graph, filters.isolate_connected, selectedNode?.id ?? null, filters.isolate_depth),
    [connectionGraph, filters.isolate_connected, filters.isolate_depth, graph, selectedNode?.id],
  );
  const visibleSelectedNode = useMemo(
    () => visibleGraph?.nodes.find((node) => selectedNode && node.id === selectedNode.id) ?? null,
    [selectedNode, visibleGraph?.nodes],
  );
  const visibleSelectedEdge = useMemo(
    () => (selectedEdge && visibleGraph?.edges.some((edge) => edge.id === selectedEdge.id) ? selectedEdge : null),
    [selectedEdge, visibleGraph?.edges],
  );
  const scopedMemoryGraph = useMemo(
    () => filterMemoryGraphForSelectedCodeNode(memoryGraph, visibleSelectedNode, memoryDepth),
    [memoryDepth, memoryGraph, visibleSelectedNode],
  );
  const renderData = useMemo(
    () =>
      buildLayeredRenderData({
        codeGraph: visibleLayers.code ? visibleGraph : null,
        memoryGraph: scopedMemoryGraph,
        visibleLayers,
        hoveredLayer,
        selectedNodeId: visibleSelectedNode?.id ?? null,
        selectedEdgeId: visibleSelectedEdge?.id ?? null,
        memorySelection,
      }),
    [hoveredLayer, memorySelection, scopedMemoryGraph, visibleGraph, visibleLayers, visibleSelectedEdge?.id, visibleSelectedNode?.id],
  );
  const memoryGraphCounts = useMemo(() => countMemoryGraphEdges(scopedMemoryGraph), [scopedMemoryGraph]);

  function handleLayerChange(layer: GraphLayer, checked: boolean) {
    const nextLayers = { ...visibleLayers, [layer]: checked };
    setVisibleLayers(nextLayers);
    if (!checked && memorySelection && !isMemorySelectionVisible(memorySelection, nextLayers)) setMemorySelection(null);
  }

  function handleSelectRenderNode(node: RenderNode, options: { shiftKey?: boolean } = {}) {
    if (node.renderKind === "code_node") {
      setMemorySelection(null);
      onSelectNode(node.id, options);
      return;
    }
    if (node.memoryNode?.node_kind === "memory") {
      onClearSelection();
      setMemorySelection({ kind: "memory_node", node: node.memoryNode });
    } else if (node.memoryNode) {
      onClearSelection();
      setMemorySelection({ kind: "source_node", node: node.memoryNode });
    }
  }

  function handleSelectRenderLink(link: RenderLink) {
    if (link.renderKind === "code_edge") {
      setMemorySelection(null);
      onSelectEdge(link.id);
      return;
    }
    if (!link.memoryEdge) return;
    onClearSelection();
    setMemorySelection({
      kind:
        link.renderKind === "provenance_edge"
          ? "provenance_edge"
          : link.renderKind === "memory_attachment_edge"
            ? "memory_attachment_edge"
            : "memory_relation_edge",
      edge: link.memoryEdge,
    });
  }

  function handleClearGraphSelection() {
    setMemorySelection(null);
    onClearSelection();
  }

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
        <div className="graph-history">
          <button type="button" onClick={onGoBackSelection} disabled={!canGoBackSelection}>
            Back
          </button>
          <button type="button" onClick={onGoForwardSelection} disabled={!canGoForwardSelection}>
            Forward
          </button>
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
            connectionView={connectionView}
            isolateConnected={filters.isolate_connected}
            isolateDepth={filters.isolate_depth}
          />
        ) : null}
        {memoryGraph ? (
          <span>
            memory graph {memoryGraph.returned_memories} memories / {memoryGraph.nodes.length} nodes /{" "}
            {memoryGraph.edges.length} edges
          </span>
        ) : null}
        {graph?.truncated ? <span className="warning-inline">{graph.truncation_reason}</span> : null}
        {error ? <span className="warning-inline">{error}</span> : null}
      </div>

      {!webglSupported ? (
        <div className="graph-empty">
          <h2>WebGL is required</h2>
          <p>This graph explorer needs a browser with WebGL enabled.</p>
        </div>
      ) : graph && !graph.status.has_graph && !(memoryGraph?.nodes.length ?? 0) ? (
        <div className="graph-empty">
          <h2>No graph data yet</h2>
          <p>Run <code>memory graph extract --project {project}</code> and refresh this tab.</p>
          <p>
            No memories at all yet? <code>memory demo</code> loads a showcase project whose memory
            graph renders here.
          </p>
        </div>
      ) : (
        <div className="graph-workspace">
          <div className="graph-scene-panel">
            <GraphScene
              renderData={renderData}
              onSelectNode={handleSelectRenderNode}
              onSelectEdge={handleSelectRenderLink}
              onClearSelection={handleClearGraphSelection}
              onHoverLayer={setHoveredLayer}
            />
            <GraphLayerControls
              visibleLayers={visibleLayers}
              hoveredLayer={hoveredLayer}
              codeNodeCount={visibleGraph?.nodes.length ?? 0}
              codeEdgeCount={visibleGraph?.edges.length ?? 0}
              provenanceEdgeCount={memoryGraphCounts.provenance}
              relationEdgeCount={memoryGraphCounts.memory_relations}
              memoryDepth={memoryDepth}
              memoryDepthDisabled={!visibleLayers.memory_relations || !visibleSelectedNode}
              onMemoryDepthChange={setMemoryDepth}
              onLayerChange={handleLayerChange}
              onHoverLayer={setHoveredLayer}
            />
          </div>
          <GraphInspector
            node={memorySelection ? null : (visibleSelectedNode as VisibleCodeGraphNode | null)}
            edge={memorySelection ? null : visibleSelectedEdge}
            graph={visibleGraph}
            memorySelection={memorySelection}
            memoryGraph={memoryGraph}
          />
        </div>
      )}
    </section>
  );
}

function GraphShowingSummary({
  graph,
  visibleGraph,
  connectionView,
  isolateConnected,
  isolateDepth,
}: {
  graph: CodeGraphResponse;
  visibleGraph: CodeGraphResponse | null;
  connectionView: GraphConnectionView | null;
  isolateConnected: boolean;
  isolateDepth: number;
}) {
  if (connectionView) {
    const source = graph.nodes.find((node) => node.id === connectionView.sourceNodeId);
    const target = graph.nodes.find((node) => node.id === connectionView.targetNodeId);
    if ((visibleGraph?.stats.returned_edges ?? 0) === 0) {
      return (
        <span>
          no connecting path in loaded graph between {source?.label ?? connectionView.sourceNodeId} and{" "}
          {target?.label ?? connectionView.targetNodeId}
        </span>
      );
    }
    return (
      <span>
        connecting {source?.label ?? connectionView.sourceNodeId} to {target?.label ?? connectionView.targetNodeId},
        showing {visibleGraph?.stats.returned_nodes ?? 0} / {visibleGraph?.stats.returned_edges ?? 0}
      </span>
    );
  }
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
  renderData,
  onSelectNode,
  onSelectEdge,
  onClearSelection,
  onHoverLayer,
}: {
  renderData: { nodes: RenderNode[]; links: RenderLink[] };
  onSelectNode: (node: RenderNode, options?: { shiftKey?: boolean }) => void;
  onSelectEdge: (edge: RenderLink) => void;
  onClearSelection: () => void;
  onHoverLayer: (layer: GraphLayer | null) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const instanceRef = useRef<ForceGraph3DInstance<RenderNode, RenderLink> | null>(null);
  const onSelectNodeRef = useRef(onSelectNode);
  const onSelectEdgeRef = useRef(onSelectEdge);
  const onClearSelectionRef = useRef(onClearSelection);
  const onHoverLayerRef = useRef(onHoverLayer);
  const lastFitSignatureRef = useRef<string | null>(null);
  const topologySignature = useMemo(() => graphRenderTopologySignature(renderData), [renderData]);

  useEffect(() => {
    onSelectNodeRef.current = onSelectNode;
    onSelectEdgeRef.current = onSelectEdge;
    onClearSelectionRef.current = onClearSelection;
    onHoverLayerRef.current = onHoverLayer;
  }, [onClearSelection, onHoverLayer, onSelectEdge, onSelectNode]);

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
      .onNodeClick((node, event) => onSelectNodeRef.current(node, { shiftKey: event.shiftKey }))
      .onLinkClick((link) => onSelectEdgeRef.current(link))
      .onNodeHover((node) => onHoverLayerRef.current(node?.primaryLayer ?? null))
      .onLinkHover((link) => onHoverLayerRef.current(link?.primaryLayer ?? null))
      .onBackgroundClick(() => {
        onClearSelectionRef.current();
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
    const shouldFit = topologySignature !== lastFitSignatureRef.current;
    lastFitSignatureRef.current = topologySignature;
    if (shouldFit) {
      instance.graphData(renderData);
      if (renderData.nodes.length) {
        window.setTimeout(() => instance.zoomToFit(500, 48), 50);
      }
    } else {
      syncGraphRenderStyles(instance, renderData);
      instance.refresh();
    }
  }, [renderData, topologySignature]);

  return <div ref={containerRef} className="graph-scene" data-testid="graph-scene" />;
}

export function graphRenderTopologySignature(renderData: { nodes: RenderNode[]; links: RenderLink[] }): string {
  const nodeIds = renderData.nodes.map((node) => node.id).sort().join(",");
  const linkIds = renderData.links.map((link) => link.id).sort().join(",");
  return `nodes:${nodeIds}|links:${linkIds}`;
}

function syncGraphRenderStyles(
  instance: ForceGraph3DInstance<RenderNode, RenderLink>,
  renderData: { nodes: RenderNode[]; links: RenderLink[] },
) {
  const currentData = instance.graphData();
  const nextNodes = new Map(renderData.nodes.map((node) => [node.id, node]));
  const nextLinks = new Map(renderData.links.map((link) => [link.id, link]));

  for (const node of currentData.nodes) {
    const nextNode = nextNodes.get(node.id);
    if (nextNode) Object.assign(node, nextNode);
  }
  for (const link of currentData.links) {
    const nextLink = nextLinks.get(link.id);
    if (!nextLink) continue;
    const { source: _source, target: _target, ...nextStyle } = nextLink;
    Object.assign(link, nextStyle);
  }
}

function GraphInspector({
  node,
  edge,
  graph,
  memorySelection,
  memoryGraph,
}: {
  node: VisibleCodeGraphNode | null;
  edge: CodeGraphEdge | null;
  graph: CodeGraphResponse | null;
  memorySelection: MemoryGraphSelection | null;
  memoryGraph: ProjectMemoryGraphResponse | null;
}) {
  if (memorySelection?.kind === "memory_node") {
    const memory = memorySelection.node;
    return (
      <aside className="graph-inspector">
        <h2>{memory.label}</h2>
        <dl>
          <dt>Kind</dt><dd>Memory</dd>
          <dt>Type</dt><dd>{memory.memory_type ?? "n/a"}</dd>
          <dt>Confidence</dt><dd>{formatScore(memory.confidence)}</dd>
          <dt>Importance</dt><dd>{memory.importance ?? "n/a"}</dd>
          <dt>Tags</dt><dd>{memory.tags.length ? memory.tags.join(", ") : "n/a"}</dd>
          <dt>Identity</dt><dd><code>{memory.memory_id ?? memory.id}</code></dd>
        </dl>
      </aside>
    );
  }
  if (memorySelection?.kind === "source_node") {
    const source = memorySelection.node;
    return (
      <aside className="graph-inspector">
        <h2>{source.label}</h2>
        <dl>
          <dt>Kind</dt><dd>Source</dd>
          <dt>Source type</dt><dd>{source.source_kind ?? "n/a"}</dd>
          <dt>File</dt><dd>{source.file_path ?? "n/a"}</dd>
          <dt>Symbol</dt><dd>{source.symbol_name ?? "n/a"}</dd>
          <dt>Symbol kind</dt><dd>{source.symbol_kind ?? "n/a"}</dd>
          <dt>Commit</dt><dd>{source.git_commit ?? "n/a"}</dd>
          <dt>Provenance</dt><dd>{source.provenance_status ?? "not checked"}</dd>
          <dt>Identity</dt><dd><code>{source.source_id ?? source.id}</code></dd>
        </dl>
      </aside>
    );
  }
  if (
    memorySelection?.kind === "provenance_edge" ||
    memorySelection?.kind === "memory_attachment_edge" ||
    memorySelection?.kind === "memory_relation_edge"
  ) {
    const selectedEdge = memorySelection.edge;
    const source = memoryGraph?.nodes.find((candidate) => candidate.id === selectedEdge.source);
    const target = memoryGraph?.nodes.find((candidate) => candidate.id === selectedEdge.target);
    return (
      <aside className="graph-inspector">
        <h2>
          {memorySelection.kind === "memory_relation_edge"
            ? selectedEdge.relation_type
            : memorySelection.kind === "memory_attachment_edge"
              ? "Memory attachment"
              : "Provenance"}
        </h2>
        <dl>
          <dt>Kind</dt><dd>{selectedEdge.edge_kind}</dd>
          <dt>Source</dt><dd>{source?.label ?? selectedEdge.source}</dd>
          <dt>Target</dt><dd>{target?.label ?? selectedEdge.target}</dd>
          <dt>Relation</dt><dd>{selectedEdge.relation_type ?? "n/a"}</dd>
          <dt>Source type</dt><dd>{selectedEdge.source_kind ?? "n/a"}</dd>
        </dl>
      </aside>
    );
  }
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
          {node.isolate_degree !== undefined ? <><dt>Distance</dt><dd>{node.isolate_degree}</dd></> : null}
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

function GraphLayerControls({
  visibleLayers,
  hoveredLayer,
  codeNodeCount,
  codeEdgeCount,
  provenanceEdgeCount,
  relationEdgeCount,
  memoryDepth,
  memoryDepthDisabled,
  onMemoryDepthChange,
  onLayerChange,
  onHoverLayer,
}: {
  visibleLayers: LayerVisibility;
  hoveredLayer: GraphLayer | null;
  codeNodeCount: number;
  codeEdgeCount: number;
  provenanceEdgeCount: number;
  relationEdgeCount: number;
  memoryDepth: number;
  memoryDepthDisabled: boolean;
  onMemoryDepthChange: (depth: number) => void;
  onLayerChange: (layer: GraphLayer, checked: boolean) => void;
  onHoverLayer: (layer: GraphLayer | null) => void;
}) {
  const normalizedMemoryDepth = normalizeMemoryDepth(memoryDepth);
  return (
    <div className="graph-layer-controls" aria-label="Graph layers">
      <LayerToggle
        layer="code"
        label="Code"
        checked={visibleLayers.code}
        active={hoveredLayer === "code"}
        count={`${codeNodeCount} nodes / ${codeEdgeCount} edges`}
        onChange={onLayerChange}
        onHover={onHoverLayer}
      />
      <LayerToggle
        layer="provenance"
        label="Provenance"
        checked={visibleLayers.provenance}
        active={hoveredLayer === "provenance"}
        count={`${provenanceEdgeCount} edges`}
        onChange={onLayerChange}
        onHover={onHoverLayer}
      />
      <div
        className="graph-layer-with-stepper"
        onMouseEnter={() => onHoverLayer("memory_relations")}
        onMouseLeave={() => onHoverLayer(null)}
      >
        <LayerToggle
          layer="memory_relations"
          label="Memory"
          checked={visibleLayers.memory_relations}
          active={hoveredLayer === "memory_relations"}
          count={`${provenanceEdgeCount + relationEdgeCount} edges`}
          onChange={onLayerChange}
          onHover={onHoverLayer}
        />
        <span className="graph-degree-stepper graph-layer-degree-stepper" aria-label="Memory degrees control">
          <button
            type="button"
            aria-label="Decrease memory degrees"
            disabled={memoryDepthDisabled || normalizedMemoryDepth <= 1}
            onClick={() => onMemoryDepthChange(normalizedMemoryDepth - 1)}
          >
            -
          </button>
          <input
            aria-label="Memory degrees"
            min={1}
            max={MAX_MEMORY_DEPTH}
            step={1}
            type="number"
            value={memoryDepth}
            disabled={memoryDepthDisabled}
            onChange={(event) => onMemoryDepthChange(normalizeMemoryDepth(Number(event.target.value)))}
          />
          <button
            type="button"
            aria-label="Increase memory degrees"
            disabled={memoryDepthDisabled || normalizedMemoryDepth >= MAX_MEMORY_DEPTH}
            onClick={() => onMemoryDepthChange(normalizedMemoryDepth + 1)}
          >
            +
          </button>
        </span>
      </div>
    </div>
  );
}

function LayerToggle({
  layer,
  label,
  checked,
  active,
  count,
  onChange,
  onHover,
}: {
  layer: GraphLayer;
  label: string;
  checked: boolean;
  active: boolean;
  count: string;
  onChange: (layer: GraphLayer, checked: boolean) => void;
  onHover: (layer: GraphLayer | null) => void;
}) {
  return (
    <label
      className={`graph-layer-toggle${active ? " active" : ""}`}
      onMouseEnter={() => onHover(layer)}
      onMouseLeave={() => onHover(null)}
    >
      <input type="checkbox" checked={checked} onChange={(event) => onChange(layer, event.target.checked)} />
      <span>{label}</span>
      <small>{count}</small>
    </label>
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

  const visibleNodeDepths = new Map<string, number>();
  const pending = [{ nodeId: anchorNodeId, depth: 0 }];
  while (pending.length) {
    const next = pending.shift();
    const nodeId = next?.nodeId;
    if (!nodeId || visibleNodeDepths.has(nodeId)) continue;
    const currentDepth = next?.depth ?? 0;
    visibleNodeDepths.set(nodeId, currentDepth);
    if ((next?.depth ?? 0) >= maxDepth) continue;
    for (const nextNodeId of adjacency.get(nodeId) ?? []) {
      if (!visibleNodeDepths.has(nextNodeId)) pending.push({ nodeId: nextNodeId, depth: currentDepth + 1 });
    }
  }

  const nodes = graph.nodes
    .filter((node) => visibleNodeDepths.has(node.id))
    .map((node) => ({ ...node, isolate_degree: visibleNodeDepths.get(node.id) ?? 0 }));
  const edges = graph.edges.filter((edge) => visibleNodeDepths.has(edge.source) && visibleNodeDepths.has(edge.target));
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

export function applyGraphConnectionView(
  graph: CodeGraphResponse | null,
  connectionView: GraphConnectionView | null,
): CodeGraphResponse | null {
  if (!graph || !connectionView) return null;

  const graphNodeIds = new Set(graph.nodes.map((node) => node.id));
  const { sourceNodeId, targetNodeId } = connectionView;
  if (!graphNodeIds.has(sourceNodeId) || !graphNodeIds.has(targetNodeId) || sourceNodeId === targetNodeId) {
    return null;
  }

  const blocks = findBiconnectedEdgeBlocks(graph);
  const routeBlocks = findBlockRoute(blocks, sourceNodeId, targetNodeId);
  const visibleEdgeIds = new Set(routeBlocks.flatMap((block) => block.edgeIds));
  const visibleNodeIds = new Set<string>([sourceNodeId, targetNodeId]);
  for (const edge of graph.edges) {
    if (!visibleEdgeIds.has(edge.id)) continue;
    visibleNodeIds.add(edge.source);
    visibleNodeIds.add(edge.target);
  }

  const nodes = graph.nodes.filter((node) => visibleNodeIds.has(node.id));
  const edges = graph.edges.filter((edge) => visibleEdgeIds.has(edge.id));
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

interface GraphEdgeBlock {
  edgeIds: string[];
  nodeIds: Set<string>;
}

function findBiconnectedEdgeBlocks(graph: CodeGraphResponse): GraphEdgeBlock[] {
  const adjacency = new Map<string, Array<{ nextNodeId: string; edge: CodeGraphEdge }>>();
  for (const node of graph.nodes) adjacency.set(node.id, []);
  for (const edge of graph.edges) {
    if (!adjacency.has(edge.source) || !adjacency.has(edge.target)) continue;
    adjacency.get(edge.source)?.push({ nextNodeId: edge.target, edge });
    adjacency.get(edge.target)?.push({ nextNodeId: edge.source, edge });
  }

  const discovery = new Map<string, number>();
  const low = new Map<string, number>();
  const edgeStack: CodeGraphEdge[] = [];
  const blocks: GraphEdgeBlock[] = [];
  let time = 0;

  const visit = (nodeId: string, parentEdgeId: string | null) => {
    discovery.set(nodeId, ++time);
    low.set(nodeId, discovery.get(nodeId) ?? time);

    for (const { nextNodeId, edge } of adjacency.get(nodeId) ?? []) {
      if (edge.id === parentEdgeId) continue;
      if (!discovery.has(nextNodeId)) {
        edgeStack.push(edge);
        visit(nextNodeId, edge.id);
        low.set(nodeId, Math.min(low.get(nodeId) ?? 0, low.get(nextNodeId) ?? 0));
        if ((low.get(nextNodeId) ?? 0) >= (discovery.get(nodeId) ?? 0)) {
          const blockEdges: CodeGraphEdge[] = [];
          let nextEdge: CodeGraphEdge | undefined;
          do {
            nextEdge = edgeStack.pop();
            if (nextEdge) blockEdges.push(nextEdge);
          } while (nextEdge && nextEdge.id !== edge.id);
          blocks.push(toGraphEdgeBlock(blockEdges));
        }
      } else if ((discovery.get(nextNodeId) ?? 0) < (discovery.get(nodeId) ?? 0)) {
        edgeStack.push(edge);
        low.set(nodeId, Math.min(low.get(nodeId) ?? 0, discovery.get(nextNodeId) ?? 0));
      }
    }
  };

  for (const node of graph.nodes) {
    if (!discovery.has(node.id)) visit(node.id, null);
  }

  return blocks;
}

function toGraphEdgeBlock(edges: CodeGraphEdge[]): GraphEdgeBlock {
  const nodeIds = new Set<string>();
  for (const edge of edges) {
    nodeIds.add(edge.source);
    nodeIds.add(edge.target);
  }
  return { edgeIds: edges.map((edge) => edge.id), nodeIds };
}

function findBlockRoute(blocks: GraphEdgeBlock[], sourceNodeId: string, targetNodeId: string): GraphEdgeBlock[] {
  const blocksByNode = new Map<string, number[]>();
  blocks.forEach((block, blockIndex) => {
    for (const nodeId of block.nodeIds) {
      const nodeBlocks = blocksByNode.get(nodeId) ?? [];
      nodeBlocks.push(blockIndex);
      blocksByNode.set(nodeId, nodeBlocks);
    }
  });

  const tree = new Map<string, Set<string>>();
  const addTreeEdge = (left: string, right: string) => {
    if (!tree.has(left)) tree.set(left, new Set());
    if (!tree.has(right)) tree.set(right, new Set());
    tree.get(left)?.add(right);
    tree.get(right)?.add(left);
  };
  blocks.forEach((block, blockIndex) => {
    const blockKey = blockTreeBlockKey(blockIndex);
    if (!tree.has(blockKey)) tree.set(blockKey, new Set());
    for (const nodeId of block.nodeIds) {
      if ((blocksByNode.get(nodeId)?.length ?? 0) > 1) {
        addTreeEdge(blockKey, blockTreeArticulationKey(nodeId));
      }
    }
  });

  const sourceKey = blockTreeKeyForNode(blocksByNode, sourceNodeId);
  const targetKey = blockTreeKeyForNode(blocksByNode, targetNodeId);
  if (!sourceKey || !targetKey) return [];

  const previous = new Map<string, string | null>();
  const pending = [sourceKey];
  previous.set(sourceKey, null);
  while (pending.length && !previous.has(targetKey)) {
    const currentKey = pending.shift();
    if (!currentKey) continue;
    for (const nextKey of tree.get(currentKey) ?? []) {
      if (previous.has(nextKey)) continue;
      previous.set(nextKey, currentKey);
      pending.push(nextKey);
    }
  }
  if (!previous.has(targetKey)) return [];

  const routeBlockIds = new Set<number>();
  let currentKey: string | null = targetKey;
  while (currentKey) {
    const blockId = parseBlockTreeBlockKey(currentKey);
    if (blockId !== null) routeBlockIds.add(blockId);
    currentKey = previous.get(currentKey) ?? null;
  }
  return [...routeBlockIds].sort((left, right) => left - right).map((blockId) => blocks[blockId]);
}

function blockTreeKeyForNode(blocksByNode: Map<string, number[]>, nodeId: string): string | null {
  const nodeBlocks = blocksByNode.get(nodeId) ?? [];
  if (!nodeBlocks.length) return null;
  return nodeBlocks.length > 1 ? blockTreeArticulationKey(nodeId) : blockTreeBlockKey(nodeBlocks[0]);
}

function blockTreeBlockKey(blockId: number): string {
  return `block:${blockId}`;
}

function blockTreeArticulationKey(nodeId: string): string {
  return `articulation:${nodeId}`;
}

function parseBlockTreeBlockKey(key: string): number | null {
  if (!key.startsWith("block:")) return null;
  const blockId = Number(key.slice("block:".length));
  return Number.isInteger(blockId) ? blockId : null;
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

function normalizeMemoryDepth(memoryDepth: number): number {
  if (!Number.isFinite(memoryDepth)) return 1;
  return Math.max(1, Math.min(MAX_MEMORY_DEPTH, Math.floor(memoryDepth)));
}

export function filterMemoryGraphForSelectedCodeNode(
  memoryGraph: ProjectMemoryGraphResponse | null,
  selectedNode: CodeGraphNode | null,
  memoryDepth: number,
): ProjectMemoryGraphResponse | null {
  if (!memoryGraph) return null;
  if (!selectedNode) return emptyScopedMemoryGraph(memoryGraph);

  const nodeIds = new Set(memoryGraph.nodes.map((node) => node.id));
  const anchorNodeIds = memoryGraph.nodes
    .filter((node) => memorySourceMatchesCodeNode(node, selectedNode))
    .map((node) => node.id);
  if (!anchorNodeIds.length) return emptyScopedMemoryGraph(memoryGraph);

  const adjacency = new Map<string, Set<string>>();
  for (const nodeId of nodeIds) adjacency.set(nodeId, new Set());
  for (const edge of memoryGraph.edges) {
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) continue;
    adjacency.get(edge.source)?.add(edge.target);
    adjacency.get(edge.target)?.add(edge.source);
  }

  const maxDepth = normalizeMemoryDepth(memoryDepth);
  const visibleNodeIds = new Set<string>();
  const pending = anchorNodeIds.map((nodeId) => ({ nodeId, depth: 0 }));
  while (pending.length) {
    const next = pending.shift();
    if (!next || visibleNodeIds.has(next.nodeId)) continue;
    visibleNodeIds.add(next.nodeId);
    if (next.depth >= maxDepth) continue;
    for (const nextNodeId of adjacency.get(next.nodeId) ?? []) {
      if (!visibleNodeIds.has(nextNodeId)) pending.push({ nodeId: nextNodeId, depth: next.depth + 1 });
    }
  }

  const nodes = memoryGraph.nodes.filter((node) => visibleNodeIds.has(node.id));
  const edges = memoryGraph.edges.filter((edge) => visibleNodeIds.has(edge.source) && visibleNodeIds.has(edge.target));
  return {
    ...memoryGraph,
    returned_memories: nodes.filter((node) => node.node_kind === "memory").length,
    nodes,
    edges,
  };
}

function emptyScopedMemoryGraph(memoryGraph: ProjectMemoryGraphResponse): ProjectMemoryGraphResponse {
  return { ...memoryGraph, returned_memories: 0, nodes: [], edges: [] };
}

function memorySourceMatchesCodeNode(sourceNode: ProjectMemoryGraphNode, codeNode: CodeGraphNode): boolean {
  if (sourceNode.node_kind !== "source") return false;
  if (!sourceNode.file_path || !codeNode.file_path || sourceNode.file_path !== codeNode.file_path) return false;

  const sourceSymbol = normalizeGraphSymbol(sourceNode.symbol_name);
  if (!sourceSymbol) return true;

  const codeSymbols = [codeNode.name, codeNode.qualified_name, codeNode.label]
    .map(normalizeGraphSymbol)
    .filter((value): value is string => Boolean(value));
  return codeSymbols.some(
    (codeSymbol) =>
      codeSymbol === sourceSymbol ||
      codeSymbol.endsWith(`.${sourceSymbol}`) ||
      codeSymbol.endsWith(`::${sourceSymbol}`) ||
      sourceSymbol.endsWith(`.${codeSymbol}`) ||
      sourceSymbol.endsWith(`::${codeSymbol}`),
  );
}

function normalizeGraphSymbol(value?: string | null): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

type VisibleCodeGraphNode = CodeGraphNode & {
  isolate_degree?: number;
};

export function buildRenderData(
  graph: CodeGraphResponse | null,
  selectedNodeId: string | null,
  selectedEdgeId: string | null,
): { nodes: RenderNode[]; links: RenderLink[] } {
  const nodes = (graph?.nodes ?? []).map((node) => {
    const selected = node.id === selectedNodeId;
    const baseValue = Math.max(3, Math.min(14, 3 + node.degree));
    return {
      ...node,
      selected,
      layers: ["code"] as GraphLayer[],
      primaryLayer: "code" as GraphLayer,
      renderKind: "code_node" as const,
      val: selected ? Math.max(18, baseValue * 1.8) : baseValue,
      color: selected
        ? "#ffffff"
        : (node as VisibleCodeGraphNode).isolate_degree !== undefined
          ? colorForIsolationDegree((node as VisibleCodeGraphNode).isolate_degree ?? 0)
          : node.seed
            ? "#7be0c5"
            : colorForGroup(node.group),
    };
  });
  const links = (graph?.edges ?? []).map((edge) => ({
    ...edge,
    layers: ["code"] as GraphLayer[],
    primaryLayer: "code" as GraphLayer,
    renderKind: "code_edge" as const,
    color: edge.id === selectedEdgeId ? "#ffc96b" : colorForEdge(edge.edge_kind),
    width: edge.id === selectedEdgeId ? 2.8 : Math.max(0.8, Math.min(2.2, edge.confidence * 2)),
  }));
  return { nodes, links };
}

export function buildLayeredRenderData({
  codeGraph,
  memoryGraph,
  visibleLayers,
  hoveredLayer,
  selectedNodeId,
  selectedEdgeId,
  memorySelection,
}: {
  codeGraph: CodeGraphResponse | null;
  memoryGraph: ProjectMemoryGraphResponse | null;
  visibleLayers: LayerVisibility;
  hoveredLayer: GraphLayer | null;
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  memorySelection: MemoryGraphSelection | null;
}): { nodes: RenderNode[]; links: RenderLink[] } {
  const codeRenderData = buildRenderData(codeGraph, selectedNodeId, selectedEdgeId);
  const nodes = codeRenderData.nodes.map((node) => applyLayerHighlight(node, hoveredLayer));
  const links = codeRenderData.links.map((link) => applyLayerHighlight(link, hoveredLayer));
  const memoryNodesById = new Map((memoryGraph?.nodes ?? []).map((node) => [node.id, node]));
  const selectedCodeAnchorId =
    selectedNodeId && codeRenderData.nodes.some((node) => node.id === selectedNodeId) ? selectedNodeId : null;

  const visibleMemoryEdges = (memoryGraph?.edges ?? []).filter((edge) =>
    edge.edge_kind === "provenance" ? visibleLayers.provenance : visibleLayers.memory_relations,
  );
  const visibleAttachmentEdges = (memoryGraph?.edges ?? []).filter(
    (edge) =>
      visibleLayers.memory_relations &&
      !visibleLayers.provenance &&
      selectedCodeAnchorId &&
      edge.edge_kind === "provenance" &&
      Boolean(memoryEndpointForProvenanceEdge(edge, memoryNodesById)),
  );
  const visibleMemoryNodeIds = new Set<string>();
  const nodeLayers = new Map<string, Set<GraphLayer>>();
  for (const edge of visibleMemoryEdges) {
    const layer = layerForMemoryEdge(edge, visibleLayers);
    visibleMemoryNodeIds.add(edge.source);
    visibleMemoryNodeIds.add(edge.target);
    addNodeLayer(nodeLayers, edge.source, layer);
    addNodeLayer(nodeLayers, edge.target, layer);
  }
  for (const edge of visibleAttachmentEdges) {
    const memoryEndpoint = memoryEndpointForProvenanceEdge(edge, memoryNodesById);
    if (!memoryEndpoint) continue;
    visibleMemoryNodeIds.add(memoryEndpoint.id);
    addNodeLayer(nodeLayers, memoryEndpoint.id, "memory_relations");
  }

  for (const node of memoryGraph?.nodes ?? []) {
    if (!visibleMemoryNodeIds.has(node.id)) continue;
    const layers = [...(nodeLayers.get(node.id) ?? new Set<GraphLayer>())];
    const primaryLayer = layers.includes("provenance") ? "provenance" : "memory_relations";
    const selected = isMemoryNodeSelection(memorySelection) && memorySelection.node.id === node.id;
    nodes.push(
      applyLayerHighlight(
        {
          id: node.id,
          label: node.label,
          selected,
          layers,
          primaryLayer,
          renderKind: node.node_kind === "memory" ? "memory_node" : "source_node",
          memoryNode: node,
          val: selected ? 16 : node.node_kind === "memory" ? 9 : 6,
          color: selected ? "#ffffff" : colorForMemoryNode(node, primaryLayer),
        },
        hoveredLayer,
      ),
    );
  }

  for (const edge of visibleMemoryEdges) {
    const layer = layerForMemoryEdge(edge, visibleLayers);
    const selected = isMemoryEdgeSelection(memorySelection) && memorySelection.edge.id === edge.id;
    links.push(
      applyLayerHighlight(
        {
          id: edge.id,
          source: edge.source,
          target: edge.target,
          layers: [layer],
          primaryLayer: layer,
          renderKind: edge.edge_kind === "provenance" ? "provenance_edge" : "memory_relation_edge",
          memoryEdge: edge,
          color: selected ? "#ffffff" : colorForMemoryEdge(edge),
          width: selected ? 3 : edge.edge_kind === "provenance" ? 1.4 : 2,
        },
        hoveredLayer,
      ),
    );
  }
  for (const edge of visibleAttachmentEdges) {
    const memoryEndpoint = memoryEndpointForProvenanceEdge(edge, memoryNodesById);
    if (!memoryEndpoint || !selectedCodeAnchorId) continue;
    const selected = isMemoryEdgeSelection(memorySelection) && memorySelection.edge.id === edge.id;
    links.push(
      applyLayerHighlight(
        {
          id: `memory-attachment:${selectedCodeAnchorId}:${edge.id}`,
          source: selectedCodeAnchorId,
          target: memoryEndpoint.id,
          layers: ["memory_relations"],
          primaryLayer: "memory_relations",
          renderKind: "memory_attachment_edge",
          memoryEdge: edge,
          color: selected ? "#ffffff" : colorForMemoryEdge(edge),
          width: selected ? 3 : 1.8,
        },
        hoveredLayer,
      ),
    );
  }

  return { nodes, links };
}

function memoryEndpointForProvenanceEdge(
  edge: ProjectMemoryGraphEdge,
  nodesById: Map<string, ProjectMemoryGraphNode>,
): ProjectMemoryGraphNode | null {
  const sourceNode = nodesById.get(edge.source);
  if (sourceNode?.node_kind === "memory") return sourceNode;
  const targetNode = nodesById.get(edge.target);
  if (targetNode?.node_kind === "memory") return targetNode;
  return null;
}

function layerForMemoryEdge(edge: ProjectMemoryGraphEdge, visibleLayers: LayerVisibility): GraphLayer {
  if (edge.edge_kind === "memory_relation") return "memory_relations";
  return visibleLayers.provenance ? "provenance" : "memory_relations";
}

function addNodeLayer(nodeLayers: Map<string, Set<GraphLayer>>, nodeId: string, layer: GraphLayer) {
  const layers = nodeLayers.get(nodeId) ?? new Set<GraphLayer>();
  layers.add(layer);
  nodeLayers.set(nodeId, layers);
}

function applyLayerHighlight<T extends { layers: GraphLayer[]; color: string; width?: number; val?: number }>(
  item: T,
  hoveredLayer: GraphLayer | null,
): T {
  if (!hoveredLayer) return item;
  if (item.layers.includes(hoveredLayer)) {
    return {
      ...item,
      color: brightenColor(item.color),
      width: item.width ? item.width * 1.45 : item.width,
      val: item.val ? item.val * 1.2 : item.val,
    };
  }
  return {
    ...item,
    color: "#2d3744",
    width: item.width ? Math.max(0.5, item.width * 0.55) : item.width,
    val: item.val ? Math.max(2, item.val * 0.75) : item.val,
  };
}

function nodeLabel(node: RenderNode): string {
  if (node.renderKind === "memory_node") {
    return `${node.label}<br/>memory ${node.memoryNode?.memory_type ?? "unknown"}<br/>confidence ${formatScore(node.memoryNode?.confidence)}`;
  }
  if (node.renderKind === "source_node") {
    return `${node.label}<br/>source ${node.memoryNode?.source_kind ?? "unknown"}<br/>${node.memoryNode?.provenance_status ?? "not checked"}`;
  }
  const location = node.file_path ? `${node.file_path}:${node.start_line ?? "?"}` : "no file";
  const distance = node.isolate_degree !== undefined ? `<br/>distance ${node.isolate_degree}` : "";
  return `${node.label}<br/>${node.symbol_kind ?? node.node_kind}<br/>${location}${distance}`;
}

function linkLabel(link: RenderLink): string {
  if (link.renderKind === "provenance_edge") {
    return `provenance<br/>${link.memoryEdge?.source_kind ?? "source"}`;
  }
  if (link.renderKind === "memory_attachment_edge") {
    return `memory attachment<br/>${link.memoryEdge?.source_kind ?? "source"}`;
  }
  if (link.renderKind === "memory_relation_edge") {
    return `memory relation<br/>${link.memoryEdge?.relation_type ?? "related"}`;
  }
  return `${link.edge_kind ?? "edge"}<br/>${link.file_path ?? "no file"}:${link.start_line ?? "?"}`;
}

function colorForGroup(group: string): string {
  const palette = ["#8be3a0", "#8ab4ff", "#d7a8ff", "#ffb082", "#f48fb1", "#9ad7d1"];
  let hash = 0;
  for (const char of group) hash = (hash * 31 + char.charCodeAt(0)) % 997;
  return palette[hash % palette.length];
}

function colorForIsolationDegree(degree: number): string {
  const palette = ["#ffc96b", "#7be0c5", "#8ab4ff", "#d7a8ff", "#ffb082", "#f48fb1", "#9ad7d1", "#c6df7e", "#c4b5fd"];
  return palette[Math.max(0, Math.floor(degree)) % palette.length];
}

function colorForEdge(edgeKind: string): string {
  if (edgeKind.includes("call")) return "#7be0c5";
  if (edgeKind.includes("test")) return "#ffc96b";
  if (edgeKind.includes("import")) return "#8ab4ff";
  return "#9cb0c6";
}

function colorForMemoryNode(node: ProjectMemoryGraphNode, layer: GraphLayer): string {
  if (node.node_kind === "source") return layer === "provenance" ? "#75d2ff" : "#9cb0c6";
  if (layer === "memory_relations") return "#f2c56b";
  return "#d7a8ff";
}

function colorForMemoryEdge(edge: ProjectMemoryGraphEdge): string {
  if (edge.edge_kind === "provenance") return "#75d2ff";
  if (edge.relation_type === "supports") return "#8be3a0";
  if (edge.relation_type === "supersedes") return "#ffc96b";
  if (edge.relation_type === "duplicates") return "#f48fb1";
  return "#d7a8ff";
}

function brightenColor(color: string): string {
  if (color === "#ffffff") return color;
  const match = /^#([0-9a-f]{6})$/i.exec(color);
  if (!match) return color;
  const value = match[1];
  const parts = [value.slice(0, 2), value.slice(2, 4), value.slice(4, 6)].map((part) =>
    Math.min(255, Math.round(Number.parseInt(part, 16) * 1.25 + 24)),
  );
  return `#${parts.map((part) => part.toString(16).padStart(2, "0")).join("")}`;
}

function countMemoryGraphEdges(memoryGraph: ProjectMemoryGraphResponse | null): { provenance: number; memory_relations: number } {
  const counts = { provenance: 0, memory_relations: 0 };
  for (const edge of memoryGraph?.edges ?? []) {
    if (edge.edge_kind === "provenance") counts.provenance += 1;
    if (edge.edge_kind === "memory_relation") counts.memory_relations += 1;
  }
  return counts;
}

function isMemorySelectionVisible(selection: MemoryGraphSelection, visibleLayers: LayerVisibility): boolean {
  if (selection.kind === "memory_relation_edge") return visibleLayers.memory_relations;
  if (selection.kind === "memory_attachment_edge") return visibleLayers.memory_relations;
  if (selection.kind === "provenance_edge") return visibleLayers.provenance;
  if (selection.kind === "source_node") return visibleLayers.provenance;
  return visibleLayers.provenance || visibleLayers.memory_relations;
}

function isMemoryNodeSelection(selection: MemoryGraphSelection | null): selection is Extract<MemoryGraphSelection, { node: ProjectMemoryGraphNode }> {
  return selection?.kind === "memory_node" || selection?.kind === "source_node";
}

function isMemoryEdgeSelection(selection: MemoryGraphSelection | null): selection is Extract<MemoryGraphSelection, { edge: ProjectMemoryGraphEdge }> {
  return (
    selection?.kind === "provenance_edge" ||
    selection?.kind === "memory_attachment_edge" ||
    selection?.kind === "memory_relation_edge"
  );
}

function formatScore(value?: number | null): string {
  return typeof value === "number" ? value.toFixed(2) : "n/a";
}

function lineRange(start?: number | null, end?: number | null): string {
  if (!start && !end) return "n/a";
  if (start === end || !end) return String(start);
  return `${start}-${end}`;
}

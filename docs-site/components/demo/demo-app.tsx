"use client";

import { type FormEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";

import { type DemoGraphLink, type DemoGraphNode, type DemoMemory, type DemoTab, demoSnapshot, demoTabs } from "./demo-data";

const backendOnlyMessage = "Demo only: this action needs a running local Memory Layer service.";

export function WebUiDemoApp() {
  const [tab, setTab] = useState<DemoTab>("memories");
  const [statusMessage, setStatusMessage] = useState(
    "Loaded a sanitized static snapshot. No backend, tokens, or local files are used.",
  );
  const [memoryFilter, setMemoryFilter] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("all");
  const [typeFilter, setTypeFilter] = useState("all");
  const [selectedMemoryId, setSelectedMemoryId] = useState(demoSnapshot.memories[0]?.id ?? "");
  const [showHistory, setShowHistory] = useState(false);
  const [queryText, setQueryText] = useState(demoSnapshot.query.question);
  const [queryRan, setQueryRan] = useState(true);
  const [selectedQueryIndex, setSelectedQueryIndex] = useState(0);
  const [selectedGraphNodeId, setSelectedGraphNodeId] = useState("code-graph-tab");
  const [graphLayers, setGraphLayers] = useState({ code: true, memory: true, provenance: false });
  const [selectedActivityIndex, setSelectedActivityIndex] = useState(0);
  const [selectedProposalIndex, setSelectedProposalIndex] = useState(0);
  const [selectedSkillIndex, setSelectedSkillIndex] = useState(0);
  const [selectedAutomationIndex, setSelectedAutomationIndex] = useState(0);
  const [bundleOptions, setBundleOptions] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(demoSnapshot.bundles.options.map((option) => [option, true])),
  );

  const selectedMemory = demoSnapshot.memories.find((memory) => memory.id === selectedMemoryId) ?? demoSnapshot.memories[0];
  const filteredMemories = useMemo(() => {
    const text = memoryFilter.trim().toLowerCase();
    const tag = tagFilter.trim().toLowerCase();
    return demoSnapshot.memories.filter((memory) => {
      const textMatch = !text || `${memory.summary} ${memory.preview} ${memory.canonicalText}`.toLowerCase().includes(text);
      const tagMatch = !tag || memory.tags.some((candidate) => candidate.toLowerCase().includes(tag));
      const statusMatch = statusFilter === "all" || memory.status === statusFilter;
      const typeMatch = typeFilter === "all" || memory.type === typeFilter;
      return textMatch && tagMatch && statusMatch && typeMatch;
    });
  }, [memoryFilter, statusFilter, tagFilter, typeFilter]);
  const memoryTypes = Array.from(new Set(demoSnapshot.memories.map((memory) => memory.type)));

  function notify(message: string) {
    setStatusMessage(message);
  }

  function handleBackendOnly(action: string) {
    notify(`${backendOnlyMessage} ${action}`);
  }

  function handleQuerySubmit(event: FormEvent) {
    event.preventDefault();
    setQueryRan(true);
    notify(
      queryText.trim()
        ? `Demo replayed a static query for "${queryText.trim()}".`
        : "Demo only: live search needs a running local Memory Layer service.",
    );
  }

  return (
    <main className="demo-shell">
      <header className="demo-topbar">
        <div>
          <p className="demo-eyebrow">Memory Layer Web demo</p>
          <h1>Explore the browser UI without a backend</h1>
          <p>
            This is a sanitized static snapshot of the local Memory project. It shows every Web UI surface and simulates
            browser-only interactions.
          </p>
        </div>
        <div className="demo-project-card">
          <span>Project</span>
          <strong>{demoSnapshot.project}</strong>
          <small>{demoSnapshot.repoRoot}</small>
        </div>
      </header>

      <section className="demo-status-strip" aria-label="Demo status">
        <span className="demo-pill demo-pill-live">static demo</span>
        <span>Web v{demoSnapshot.version}</span>
        <span>Service simulated</span>
        <span>{demoSnapshot.overview.activeMemories} memories</span>
        <span>{demoSnapshot.overview.captures} captures</span>
        <span>{demoSnapshot.overview.graphNodes} graph nodes</span>
        <span>{demoSnapshot.overview.proposals} proposals</span>
      </section>

      <nav className="demo-tabs" aria-label="Web UI demo tabs">
        {demoTabs.map((item) => (
          <button
            key={item}
            className={tab === item ? "demo-tab-active" : ""}
            type="button"
            onClick={() => {
              setTab(item);
              notify(`Opened ${item}. This tab is backed by the static demo snapshot.`);
            }}
          >
            {item}
          </button>
        ))}
      </nav>

      <section className="demo-content">
        {tab === "memories" ? (
          <MemoriesDemo
            filteredMemories={filteredMemories}
            memoryTypes={memoryTypes}
            selectedMemory={selectedMemory}
            selectedMemoryId={selectedMemoryId}
            showHistory={showHistory}
            textFilter={memoryFilter}
            tagFilter={tagFilter}
            statusFilter={statusFilter}
            typeFilter={typeFilter}
            onTextFilterChange={setMemoryFilter}
            onTagFilterChange={setTagFilter}
            onStatusFilterChange={setStatusFilter}
            onTypeFilterChange={setTypeFilter}
            onSelectMemory={(memoryId) => {
              setSelectedMemoryId(memoryId);
              setShowHistory(false);
            }}
            onHistory={() => {
              setShowHistory((current) => !current);
              notify("Demo shows a static two-version memory history.");
            }}
            onBackendOnly={handleBackendOnly}
          />
        ) : null}
        {tab === "agents" ? <AgentsDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "query" ? (
          <QueryDemo
            queryText={queryText}
            queryRan={queryRan}
            selectedQueryIndex={selectedQueryIndex}
            onQueryTextChange={setQueryText}
            onQuerySubmit={handleQuerySubmit}
            onSelectQueryResult={setSelectedQueryIndex}
            onOpenGraph={(memoryId) => {
              const memory = demoSnapshot.memories.find((item) => item.id === memoryId);
              setTab("graph");
              setSelectedGraphNodeId(memoryId === "mem-docs-site" ? "memory-docs" : "memory-radius");
              notify(`Opened a static graph seed for ${memory?.summary ?? memoryId}.`);
            }}
            onBackendOnly={handleBackendOnly}
          />
        ) : null}
        {tab === "graph" ? (
          <GraphDemo
            selectedNodeId={selectedGraphNodeId}
            layers={graphLayers}
            onSelectNode={setSelectedGraphNodeId}
            onLayerChange={(layer, checked) => setGraphLayers((current) => ({ ...current, [layer]: checked }))}
            onBackendOnly={handleBackendOnly}
          />
        ) : null}
        {tab === "activity" ? (
          <ActivityDemo selectedIndex={selectedActivityIndex} onSelect={setSelectedActivityIndex} onBackendOnly={handleBackendOnly} />
        ) : null}
        {tab === "errors" ? <ErrorsDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "project" ? <ProjectDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "review" ? (
          <ReviewDemo
            selectedIndex={selectedProposalIndex}
            onSelect={setSelectedProposalIndex}
            onBackendOnly={handleBackendOnly}
          />
        ) : null}
        {tab === "watchers" ? <WatchersDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "skills" ? (
          <SkillsDemo selectedIndex={selectedSkillIndex} onSelect={setSelectedSkillIndex} onBackendOnly={handleBackendOnly} />
        ) : null}
        {tab === "embeddings" ? <EmbeddingsDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "resume" ? <ResumeDemo onBackendOnly={handleBackendOnly} /> : null}
        {tab === "automations" ? (
          <AutomationsDemo
            selectedIndex={selectedAutomationIndex}
            onSelect={setSelectedAutomationIndex}
            onBackendOnly={handleBackendOnly}
          />
        ) : null}
        {tab === "bundles" ? (
          <BundlesDemo options={bundleOptions} onOptionsChange={setBundleOptions} onBackendOnly={handleBackendOnly} />
        ) : null}
      </section>

      <footer className="demo-statusbar">{statusMessage}</footer>
    </main>
  );
}

function MemoriesDemo({
  filteredMemories,
  memoryTypes,
  selectedMemory,
  selectedMemoryId,
  showHistory,
  textFilter,
  tagFilter,
  statusFilter,
  typeFilter,
  onTextFilterChange,
  onTagFilterChange,
  onStatusFilterChange,
  onTypeFilterChange,
  onSelectMemory,
  onHistory,
  onBackendOnly,
}: {
  filteredMemories: DemoMemory[];
  memoryTypes: string[];
  selectedMemory: DemoMemory;
  selectedMemoryId: string;
  showHistory: boolean;
  textFilter: string;
  tagFilter: string;
  statusFilter: string;
  typeFilter: string;
  onTextFilterChange: (value: string) => void;
  onTagFilterChange: (value: string) => void;
  onStatusFilterChange: (value: string) => void;
  onTypeFilterChange: (value: string) => void;
  onSelectMemory: (memoryId: string) => void;
  onHistory: () => void;
  onBackendOnly: (action: string) => void;
}) {
  return (
    <div className="demo-panel-grid">
      <section className="demo-panel">
        <div className="demo-toolbar demo-filter-grid">
          <input placeholder="Search summary or preview" value={textFilter} onChange={(event) => onTextFilterChange(event.target.value)} />
          <input placeholder="Filter tag" value={tagFilter} onChange={(event) => onTagFilterChange(event.target.value)} />
          <select value={statusFilter} onChange={(event) => onStatusFilterChange(event.target.value)}>
            <option value="all">All statuses</option>
            <option value="active">Active</option>
            <option value="archived">Archived</option>
          </select>
          <select value={typeFilter} onChange={(event) => onTypeFilterChange(event.target.value)}>
            <option value="all">All types</option>
            {memoryTypes.map((type) => (
              <option key={type} value={type}>{type}</option>
            ))}
          </select>
        </div>
        <div className="demo-list">
          {filteredMemories.map((memory) => (
            <button
              key={memory.id}
              className={selectedMemoryId === memory.id ? "demo-list-item demo-selected" : "demo-list-item"}
              type="button"
              onClick={() => onSelectMemory(memory.id)}
            >
              <span>
                <strong>{memory.summary}</strong>
                <small>{memory.preview}</small>
              </span>
              <span className="demo-meta">
                <span className="demo-badge">{memory.type}</span>
                <span className={`demo-badge demo-badge-${memory.status}`}>{memory.status}</span>
                <span>{memory.confidence.toFixed(2)}</span>
              </span>
            </button>
          ))}
        </div>
      </section>
      <section className="demo-panel demo-detail">
        <div className="demo-detail-header">
          <div>
            <h2>{showHistory ? "Version history" : selectedMemory.summary}</h2>
            <p>
              {selectedMemory.type} - {selectedMemory.status} - confidence {selectedMemory.confidence.toFixed(2)} -
              importance {selectedMemory.importance}
            </p>
          </div>
          <div className="demo-actions">
            <button type="button" onClick={onHistory}>{showHistory ? "Hide history" : "History"}</button>
            <button type="button" className="demo-danger" onClick={() => onBackendOnly("Deleting memories is disabled in the public demo.")}>
              Delete
            </button>
          </div>
        </div>
        {showHistory ? (
          <div className="demo-card-stack">
            <MiniCard title="v2 current" text={selectedMemory.canonicalText} />
            <MiniCard title="v1 original capture" text={selectedMemory.preview} />
          </div>
        ) : (
          <>
            <MiniCard title="Canonical text" text={selectedMemory.canonicalText} />
            <section className="demo-section">
              <h3>Tags</h3>
              <div className="demo-tags">{selectedMemory.tags.map((tag) => <span key={tag}>{tag}</span>)}</div>
            </section>
            <section className="demo-section">
              <h3>Sources</h3>
              {selectedMemory.sources.map((source) => (
                <div className="demo-source" key={`${source.kind}-${source.path}`}>
                  <strong>{source.kind}</strong>
                  <span>{source.path}</span>
                  <pre>{source.excerpt}</pre>
                </div>
              ))}
            </section>
            <section className="demo-section">
              <h3>Related memories</h3>
              {selectedMemory.related.length ? selectedMemory.related.map((related) => (
                <p key={related.summary}><span className="demo-badge">{related.relation}</span> {related.summary}</p>
              )) : <p className="demo-muted">No related memories recorded.</p>}
            </section>
          </>
        )}
      </section>
    </div>
  );
}

function QueryDemo({
  queryText,
  queryRan,
  selectedQueryIndex,
  onQueryTextChange,
  onQuerySubmit,
  onSelectQueryResult,
  onOpenGraph,
  onBackendOnly,
}: {
  queryText: string;
  queryRan: boolean;
  selectedQueryIndex: number;
  onQueryTextChange: (value: string) => void;
  onQuerySubmit: (event: FormEvent) => void;
  onSelectQueryResult: (index: number) => void;
  onOpenGraph: (memoryId: string) => void;
  onBackendOnly: (action: string) => void;
}) {
  const result = demoSnapshot.query.results[selectedQueryIndex] ?? demoSnapshot.query.results[0];
  const memory = demoSnapshot.memories.find((item) => item.id === result.memoryId) ?? demoSnapshot.memories[0];
  return (
    <div className="demo-stack">
      <form className="demo-panel" onSubmit={onQuerySubmit}>
        <div className="demo-toolbar">
          <input className="demo-query-input" value={queryText} onChange={(event) => onQueryTextChange(event.target.value)} />
          <button type="submit">Query</button>
        </div>
        <label className="demo-check-row"><input type="checkbox" onChange={() => onBackendOnly("Stale ranking can be toggled locally, but live ranking requires the service.")} /> Include stale ranking</label>
        {queryRan ? (
          <div className="demo-answer">
            <p>{demoSnapshot.query.answer}</p>
            <div className="demo-stat-row">
              <span>confidence {demoSnapshot.query.confidence.toFixed(2)}</span>
              {demoSnapshot.query.diagnostics.map((line) => <span key={line}>{line}</span>)}
            </div>
          </div>
        ) : <p className="demo-muted">Run a query to inspect returned memories and diagnostics.</p>}
      </form>
      <div className="demo-panel-grid">
        <section className="demo-panel">
          <div className="demo-list">
            {demoSnapshot.query.results.map((item, index) => {
              const itemMemory = demoSnapshot.memories.find((candidate) => candidate.id === item.memoryId);
              return (
                <button
                  key={item.memoryId}
                  className={selectedQueryIndex === index ? "demo-list-item demo-selected" : "demo-list-item"}
                  type="button"
                  onClick={() => onSelectQueryResult(index)}
                >
                  <span><strong>{itemMemory?.summary}</strong><small>{itemMemory?.preview}</small></span>
                  <span className="demo-meta">
                    <span className="demo-badge">#{index + 1}</span>
                    <span className="demo-badge">{item.match}</span>
                    {item.cited ? <span className="demo-badge demo-badge-active">cited</span> : null}
                    <span>{item.score.toFixed(2)}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </section>
        <section className="demo-panel demo-detail">
          <div className="demo-detail-header">
            <div>
              <h2>{memory.summary}</h2>
              <p>{result.match} - score {result.score.toFixed(2)}</p>
            </div>
            <button type="button" onClick={() => onOpenGraph(memory.id)}>Open in Graph</button>
          </div>
          <MiniCard title="Why it ranked" text="tag match x2; source path match x3; graph boost 2.50; relation boost 19.76; updated recently" />
          <MiniCard title="Snippet" text={memory.preview} />
        </section>
      </div>
    </div>
  );
}

function GraphDemo({
  selectedNodeId,
  layers,
  onSelectNode,
  onLayerChange,
  onBackendOnly,
}: {
  selectedNodeId: string;
  layers: Record<"code" | "memory" | "provenance", boolean>;
  onSelectNode: (nodeId: string) => void;
  onLayerChange: (layer: "code" | "memory" | "provenance", checked: boolean) => void;
  onBackendOnly: (action: string) => void;
}) {
  const selectedNode = demoSnapshot.graph.nodes.find((node) => node.id === selectedNodeId) ?? demoSnapshot.graph.nodes[0];
  return (
    <div className="demo-graph-layout">
      <section className="demo-panel demo-graph-panel">
        <div className="demo-toolbar">
          <button type="button" onClick={() => onBackendOnly("Refreshing graph data requires the service graph endpoint.")}>Refresh</button>
          <button type="button" onClick={() => onBackendOnly("Graph extraction runs in the local CLI, not on the public website.")}>Extract graph</button>
          {(["code", "memory", "provenance"] as const).map((layer) => (
            <label className="demo-check-row" key={layer}>
              <input checked={layers[layer]} type="checkbox" onChange={(event) => onLayerChange(layer, event.target.checked)} />
              {layer}
            </label>
          ))}
        </div>
        <DemoForceGraph selectedNodeId={selectedNodeId} layers={layers} onSelectNode={onSelectNode} />
      </section>
      <section className="demo-panel demo-detail">
        <h2>{selectedNode.label}</h2>
        <p>{selectedNode.kind} - {selectedNode.group}</p>
        <MiniCard title="Detail" text={selectedNode.detail} />
        <MiniCard title="Layer behavior" text="Memory attachment edges are browser-only demo links from code nodes to directly attached memories. Provenance shows the underlying source evidence." />
      </section>
    </div>
  );
}

function DemoForceGraph({
  selectedNodeId,
  layers,
  onSelectNode,
}: {
  selectedNodeId: string;
  layers: Record<"code" | "memory" | "provenance", boolean>;
  onSelectNode: (nodeId: string) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const rendererRef = useRef<any>(null);
  const sceneRef = useRef<any>(null);
  const cameraRef = useRef<any>(null);
  const graphGroupRef = useRef<any>(null);
  const threeRef = useRef<typeof import("three") | null>(null);
  const frameRef = useRef<number | null>(null);
  const dragRef = useRef({ active: false, x: 0, y: 0 });
  const onSelectNodeRef = useRef(onSelectNode);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const [graphReady, setGraphReady] = useState(0);
  const [graphError, setGraphError] = useState<string | null>(null);
  onSelectNodeRef.current = onSelectNode;
  const graphData = useMemo(() => visibleGraphData(layers), [layers]);
  const selectedNode = demoSnapshot.graph.nodes.find((node) => node.id === selectedNodeId);
  const hoveredNode = hoveredNodeId ? demoSnapshot.graph.nodes.find((node) => node.id === hoveredNodeId) : null;

  useEffect(() => {
    let disposed = false;
    void import("three").then((three) => {
      if (disposed || !containerRef.current) return;
      threeRef.current = three;
      const scene = new three.Scene();
      scene.background = new three.Color("#081019");
      sceneRef.current = scene;

      const camera = new three.PerspectiveCamera(48, 1, 0.1, 2000);
      camera.position.set(0, 0, 92);
      cameraRef.current = camera;

      let renderer: import("three").WebGLRenderer;
      try {
        renderer = new three.WebGLRenderer({ antialias: true, alpha: false, preserveDrawingBuffer: true });
      } catch {
        setGraphError("WebGL is unavailable in this browser session. Enable hardware acceleration or use a WebGL-capable browser.");
        return;
      }
      setGraphError(null);
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
      rendererRef.current = renderer;
      containerRef.current.appendChild(renderer.domElement);

      const graphGroup = new three.Group();
      graphGroupRef.current = graphGroup;
      scene.add(graphGroup);
      scene.add(new three.AmbientLight("#b9d9ff", 1.2));
      const keyLight = new three.DirectionalLight("#ffffff", 2.1);
      keyLight.position.set(32, 42, 62);
      scene.add(keyLight);
      const fillLight = new three.PointLight("#7be0c5", 1.4, 180);
      fillLight.position.set(-42, -28, 50);
      scene.add(fillLight);

      const raycaster = new three.Raycaster();
      const pointer = new three.Vector2();
      const pickNode = (event: PointerEvent) => {
        const element = renderer.domElement;
        const rect = element.getBoundingClientRect();
        pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
        pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
        raycaster.setFromCamera(pointer, camera);
        const hits = raycaster.intersectObjects(graphGroup.children, true);
        const hit = hits.find((item) => item.object.userData?.nodeId);
        return hit?.object.userData?.nodeId as string | undefined;
      };
      const onPointerMove = (event: PointerEvent) => {
        if (dragRef.current.active) {
          const dx = event.clientX - dragRef.current.x;
          const dy = event.clientY - dragRef.current.y;
          dragRef.current = { active: true, x: event.clientX, y: event.clientY };
          graphGroup.rotation.y += dx * 0.006;
          graphGroup.rotation.x = Math.max(-0.85, Math.min(0.85, graphGroup.rotation.x + dy * 0.004));
          return;
        }
        const nodeId = pickNode(event) ?? null;
        setHoveredNodeId(nodeId);
        renderer.domElement.style.cursor = nodeId ? "pointer" : "grab";
      };
      const onPointerDown = (event: PointerEvent) => {
        dragRef.current = { active: true, x: event.clientX, y: event.clientY };
        renderer.domElement.style.cursor = "grabbing";
      };
      const onPointerUp = (event: PointerEvent) => {
        const moved = Math.abs(event.clientX - dragRef.current.x) + Math.abs(event.clientY - dragRef.current.y);
        dragRef.current.active = false;
        renderer.domElement.style.cursor = "grab";
        if (moved < 5) {
          const nodeId = pickNode(event);
          if (nodeId) onSelectNodeRef.current(nodeId);
        }
      };
      const onPointerLeave = () => {
        dragRef.current.active = false;
        setHoveredNodeId(null);
      };
      const onWheel = (event: WheelEvent) => {
        event.preventDefault();
        camera.position.z = Math.max(42, Math.min(150, camera.position.z + event.deltaY * 0.05));
      };

      const resize = () => {
        const element = containerRef.current;
        if (!element) return;
        const width = element.clientWidth || 900;
        const height = element.clientHeight || 620;
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
        renderer.setSize(width, height, false);
      };
      resize();
      window.addEventListener("resize", resize);
      renderer.domElement.addEventListener("pointermove", onPointerMove);
      renderer.domElement.addEventListener("pointerdown", onPointerDown);
      renderer.domElement.addEventListener("pointerup", onPointerUp);
      renderer.domElement.addEventListener("pointerleave", onPointerLeave);
      renderer.domElement.addEventListener("wheel", onWheel, { passive: false });

      const animate = () => {
        if (disposed) return;
        if (!dragRef.current.active) graphGroup.rotation.y += 0.0012;
        renderer.render(scene, camera);
        frameRef.current = window.requestAnimationFrame(animate);
      };
      renderDemoGraph(three, graphGroup, graphData, selectedNodeId, null);
      setGraphReady((current) => current + 1);
      animate();

      (renderer as any).__memoryDemoCleanup = () => {
        window.removeEventListener("resize", resize);
        renderer.domElement.removeEventListener("pointermove", onPointerMove);
        renderer.domElement.removeEventListener("pointerdown", onPointerDown);
        renderer.domElement.removeEventListener("pointerup", onPointerUp);
        renderer.domElement.removeEventListener("pointerleave", onPointerLeave);
        renderer.domElement.removeEventListener("wheel", onWheel);
      };
    });
    return () => {
      disposed = true;
      if (frameRef.current) window.cancelAnimationFrame(frameRef.current);
      rendererRef.current?.__memoryDemoCleanup?.();
      disposeGraphObjects(graphGroupRef.current);
      rendererRef.current?.dispose?.();
      rendererRef.current = null;
      sceneRef.current = null;
      cameraRef.current = null;
      graphGroupRef.current = null;
      threeRef.current = null;
      containerRef.current?.replaceChildren();
    };
  }, []);

  useEffect(() => {
    const three = threeRef.current;
    const graphGroup = graphGroupRef.current;
    if (!three || !graphGroup) return;
    renderDemoGraph(three, graphGroup, graphData, selectedNodeId, hoveredNodeId);
  }, [graphData, graphReady, hoveredNodeId, selectedNodeId]);

  return (
    <div className="demo-force-graph-wrap">
      <div ref={containerRef} className="demo-force-graph" />
      <div className="demo-graph-overlay" aria-live="polite">
        <strong>{graphError ? "WebGL unavailable" : hoveredNode?.label ?? selectedNode?.label ?? "Memory graph"}</strong>
        <span>{graphError ?? hoveredNode?.detail ?? selectedNode?.detail ?? "Drag to rotate, scroll to zoom, click a node for details."}</span>
      </div>
    </div>
  );
}

function visibleGraphData(layers: Record<"code" | "memory" | "provenance", boolean>) {
  const links = demoSnapshot.graph.links.filter((link) => {
    if (link.kind === "code") return layers.code;
    if (link.kind === "provenance") return layers.provenance;
    return layers.memory;
  });
  const nodeIds = new Set<string>();
  for (const link of links) {
    nodeIds.add(link.source);
    nodeIds.add(link.target);
  }
  const nodes = demoSnapshot.graph.nodes.filter((node) => nodeIds.has(node.id));
  return { nodes, links };
}

function renderDemoGraph(
  three: typeof import("three"),
  graphGroup: any,
  graphData: { nodes: DemoGraphNode[]; links: DemoGraphLink[] },
  selectedNodeId: string,
  hoveredNodeId: string | null,
) {
  disposeGraphObjects(graphGroup);
  const positions = layoutDemoGraphNodes(three, graphData.nodes);
  for (const link of graphData.links) {
    const source = positions.get(link.source);
    const target = positions.get(link.target);
    if (!source || !target) continue;
    const geometry = new three.BufferGeometry().setFromPoints([source, target]);
    const material = new three.LineBasicMaterial({
      color: new three.Color(colorForDemoLink(link)),
      transparent: true,
      opacity: hoveredNodeId && (link.source === hoveredNodeId || link.target === hoveredNodeId) ? 0.95 : 0.5,
    });
    const line = new three.Line(geometry, material);
    line.userData = { linkId: link.id };
    graphGroup.add(line);
  }
  for (const node of graphData.nodes) {
    const position = positions.get(node.id);
    if (!position) continue;
    const selected = node.id === selectedNodeId;
    const hovered = node.id === hoveredNodeId;
    const radius = selected ? 3.9 : hovered ? 3.45 : node.kind === "memory" ? 2.75 : node.kind === "source" ? 2.35 : 2.55;
    const geometry = new three.SphereGeometry(radius, 32, 18);
    const material = new three.MeshStandardMaterial({
      color: new three.Color(colorForDemoNode(node, selectedNodeId, hoveredNodeId)),
      emissive: new three.Color(node.kind === "memory" ? "#5f3b00" : node.kind === "source" ? "#003750" : "#003d32"),
      emissiveIntensity: selected || hovered ? 0.72 : 0.32,
      roughness: 0.36,
      metalness: 0.12,
    });
    const mesh = new three.Mesh(geometry, material);
    mesh.position.copy(position);
    mesh.userData = { nodeId: node.id };
    graphGroup.add(mesh);

    if (selected || hovered) {
      const ringGeometry = new three.TorusGeometry(radius + 0.72, 0.08, 10, 42);
      const ringMaterial = new three.MeshBasicMaterial({ color: "#ffffff", transparent: true, opacity: selected ? 0.9 : 0.62 });
      const ring = new three.Mesh(ringGeometry, ringMaterial);
      ring.position.copy(position);
      ring.lookAt(0, 0, 120);
      graphGroup.add(ring);
    }
  }
}

function layoutDemoGraphNodes(three: typeof import("three"), nodes: DemoGraphNode[]) {
  const byGroup = nodes.reduce<Record<string, DemoGraphNode[]>>((groups, node) => {
    const key = node.kind === "memory" ? "memory" : node.kind === "source" ? "source" : node.group;
    groups[key] = [...(groups[key] ?? []), node];
    return groups;
  }, {});
  const groups = Object.entries(byGroup);
  const positions = new Map<string, InstanceType<typeof three.Vector3>>();
  groups.forEach(([group, groupNodes], groupIndex) => {
    const groupAngle = (groupIndex / Math.max(groups.length, 1)) * Math.PI * 2;
    const groupRadius = group === "memory" ? 18 : group === "source" ? 34 : 26;
    const groupCenter = new three.Vector3(
      Math.cos(groupAngle) * groupRadius,
      Math.sin(groupAngle) * groupRadius,
      (groupIndex % 3 - 1) * 9,
    );
    groupNodes.forEach((node, nodeIndex) => {
      const localAngle = (nodeIndex / Math.max(groupNodes.length, 1)) * Math.PI * 2;
      const localRadius = 5 + Math.min(groupNodes.length, 8) * 0.95;
      positions.set(
        node.id,
        new three.Vector3(
          groupCenter.x + Math.cos(localAngle) * localRadius,
          groupCenter.y + Math.sin(localAngle) * localRadius,
          groupCenter.z + (nodeIndex % 4 - 1.5) * 4.5,
        ),
      );
    });
  });
  return positions;
}

function disposeGraphObjects(group: any) {
  if (!group) return;
  for (const child of [...group.children]) {
    child.geometry?.dispose?.();
    if (Array.isArray(child.material)) {
      child.material.forEach((material: any) => material.dispose?.());
    } else {
      child.material?.dispose?.();
    }
    group.remove(child);
  }
}

function colorForDemoNode(node: DemoGraphNode, selectedNodeId: string, hoveredNodeId: string | null): string {
  if (node.id === selectedNodeId) return "#ffffff";
  if (node.id === hoveredNodeId) return "#fff2b8";
  if (node.kind === "memory") return "#f2c56b";
  if (node.kind === "source") return "#75d2ff";
  return node.group === "api" ? "#d7a8ff" : "#7be0c5";
}

function colorForDemoLink(link: DemoGraphLink): string {
  if (link.kind === "attachment") return "#f2c56b";
  if (link.kind === "provenance") return "#75d2ff";
  if (link.kind === "memory") return "#d7a8ff";
  return "#8ab4ff";
}

function AgentsDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <DemoTwoPane
      items={demoSnapshot.agents}
      title={(agent) => agent.name}
      subtitle={(agent) => `${agent.status} - ${agent.tokens} - context ${agent.contextPressure}`}
      detail={(agent) => (
        <>
          <MiniCard title="Session" text={`${agent.session} in ${agent.project}`} />
          <MiniCard title="Processes" text={agent.childProcesses.join(", ") || "No child processes"} />
          <MiniCard title="Ports and limits" text={`${agent.ports.join(", ") || "No ports"} - rate limit ${agent.rateLimit}`} />
          <button type="button" onClick={() => onBackendOnly("Agent process inspection requires local runtime access.")}>Refresh agents</button>
        </>
      )}
    />
  );
}

function ActivityDemo({ selectedIndex, onSelect, onBackendOnly }: { selectedIndex: number; onSelect: (index: number) => void; onBackendOnly: (action: string) => void }) {
  const active = demoSnapshot.activities[selectedIndex] ?? demoSnapshot.activities[0];
  return (
    <DemoIndexedTwoPane
      items={demoSnapshot.activities}
      selectedIndex={selectedIndex}
      onSelect={onSelect}
      title={(activity) => activity.summary}
      subtitle={(activity) => `${activity.kind} - ${activity.duration} - ${activity.tokens}`}
      detail={() => (
        <>
          <MiniCard title="Activity details" text={`${active.summary}. Persisted activity includes duration, token use, source, and optional details.`} />
          <button type="button" onClick={() => onBackendOnly("Generating a fresh get-up-to-speed briefing requires the local service.")}>Get up to speed</button>
        </>
      )}
    />
  );
}

function ErrorsDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Diagnostics</h2><p>Persisted and browser-session errors with concrete repair hints.</p></div>
        <button type="button" onClick={() => onBackendOnly("Running memory doctor requires local CLI access.")}>Run doctor</button>
      </div>
      <div className="demo-card-stack">
        {demoSnapshot.errors.map((error) => (
          <MiniCard key={error.code} title={`${error.severity}: ${error.code}`} text={`${error.message} Fix: ${error.fix}`} />
        ))}
      </div>
    </section>
  );
}

function ProjectDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Project overview</h2><p>Counts, embeddings, proposals, watchers, graph status, and recent work.</p></div>
        <div className="demo-actions">
          <button type="button" onClick={() => onBackendOnly("Curating captures requires the local service.")}>Curate</button>
          <button type="button" onClick={() => onBackendOnly("Archiving memories is disabled in the public demo.")}>Archive</button>
        </div>
      </div>
      <MetricGrid
        metrics={[
          ["Active memories", demoSnapshot.overview.activeMemories],
          ["Raw captures", demoSnapshot.overview.captures],
          ["Recent memories", demoSnapshot.overview.recentMemories],
          ["Embedding chunks", demoSnapshot.overview.embeddingChunks],
          ["Pending proposals", demoSnapshot.overview.proposals],
          ["Watchers", demoSnapshot.overview.watchers],
          ["Graph nodes", demoSnapshot.overview.graphNodes],
          ["Graph edges", demoSnapshot.overview.graphEdges],
        ]}
      />
    </section>
  );
}

function ReviewDemo({ selectedIndex, onSelect, onBackendOnly }: { selectedIndex: number; onSelect: (index: number) => void; onBackendOnly: (action: string) => void }) {
  const proposal = demoSnapshot.proposals[selectedIndex] ?? demoSnapshot.proposals[0];
  return (
    <DemoIndexedTwoPane
      items={demoSnapshot.proposals}
      selectedIndex={selectedIndex}
      onSelect={onSelect}
      title={(item) => item.target}
      subtitle={(item) => `${item.status} - ${item.policy}`}
      detail={() => (
        <>
          <MiniCard title="Why proposed" text={proposal.reason} />
          <MiniCard title="Candidate memory" text={proposal.candidate} />
          <div className="demo-actions">
            <button type="button" onClick={() => onBackendOnly("Approving proposals mutates project memory and needs the service.")}>Approve</button>
            <button type="button" onClick={() => onBackendOnly("Rejecting proposals mutates project memory and needs the service.")}>Reject</button>
          </div>
        </>
      )}
    />
  );
}

function WatchersDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Watchers</h2><p>Presence, heartbeat, owners, and recovery state.</p></div>
        <button type="button" onClick={() => onBackendOnly("Starting or restarting watchers requires the local manager.")}>Restart watcher</button>
      </div>
      <div className="demo-grid-cards">
        {demoSnapshot.watchers.map((watcher) => (
          <MiniCard key={watcher.repo} title={`${watcher.repo} - ${watcher.state}`} text={`heartbeat ${watcher.heartbeat}; owner ${watcher.owner}; restarts ${watcher.restarts}`} />
        ))}
      </div>
    </section>
  );
}

function SkillsDemo({ selectedIndex, onSelect, onBackendOnly }: { selectedIndex: number; onSelect: (index: number) => void; onBackendOnly: (action: string) => void }) {
  const skill = demoSnapshot.skills[selectedIndex] ?? demoSnapshot.skills[0];
  return (
    <DemoIndexedTwoPane
      items={demoSnapshot.skills}
      selectedIndex={selectedIndex}
      onSelect={onSelect}
      title={(item) => item.name}
      subtitle={(item) => `${item.status} - v${item.version}`}
      detail={() => (
        <>
          <MiniCard title="Location" text={skill.path} />
          <MiniCard title="SKILL.md preview" text={`${skill.name} provides repo-local Memory Layer workflow instructions and helper command routing.`} />
          <button type="button" onClick={() => onBackendOnly("Repairing skills downloads files and is disabled in the public demo.")}>Repair skills</button>
        </>
      )}
    />
  );
}

function EmbeddingsDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Embeddings</h2><p>Configured backends, active search, automatic creation, and coverage.</p></div>
        <div className="demo-actions">
          <button type="button" onClick={() => onBackendOnly("Re-embedding requires configured providers and the local service.")}>Re-embed</button>
          <button type="button" onClick={() => onBackendOnly("Reindexing requires the local service.")}>Reindex</button>
        </div>
      </div>
      <div className="demo-grid-cards">
        {demoSnapshot.embeddings.map((backend) => (
          <MiniCard
            key={`${backend.provider}-${backend.model}`}
            title={`${backend.provider} / ${backend.model}`}
            text={`search ${backend.search ? "on" : "off"}; creation ${backend.creation ? "on" : "off"}; fresh ${backend.fresh}; stale ${backend.stale}`}
          />
        ))}
      </div>
    </section>
  );
}

function ResumeDemo({ onBackendOnly }: { onBackendOnly: (action: string) => void }) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Resume briefing</h2><p>Handoff context for returning agents.</p></div>
        <button type="button" onClick={() => onBackendOnly("Generating a fresh resume briefing requires project activities and memories.")}>Refresh briefing</button>
      </div>
      <MiniCard title="Briefing" text={demoSnapshot.resume.briefing} />
      <section className="demo-section">
        <h3>Next actions</h3>
        <ul>{demoSnapshot.resume.next.map((item) => <li key={item}>{item}</li>)}</ul>
      </section>
      <section className="demo-section">
        <h3>Warnings</h3>
        <ul>{demoSnapshot.resume.warnings.map((item) => <li key={item}>{item}</li>)}</ul>
      </section>
    </section>
  );
}

function AutomationsDemo({ selectedIndex, onSelect, onBackendOnly }: { selectedIndex: number; onSelect: (index: number) => void; onBackendOnly: (action: string) => void }) {
  const automation = demoSnapshot.automations[selectedIndex] ?? demoSnapshot.automations[0];
  return (
    <DemoIndexedTwoPane
      items={demoSnapshot.automations}
      selectedIndex={selectedIndex}
      onSelect={onSelect}
      title={(item) => item.name}
      subtitle={(item) => `${item.mode} - ${item.state} - ${item.run}`}
      detail={() => (
        <>
          <MiniCard title="Effective settings" text={`risk ${automation.risk}; mode ${automation.mode}; approvals ${automation.approvals}; latest run ${automation.run}`} />
          <MiniCard title="Context pack" text="Static demo context includes recent memories, graph references, policy decisions, and proposed output." />
          <div className="demo-actions">
            <button type="button" onClick={() => onBackendOnly("Running loops requires the local automation control plane.")}>Run loop</button>
            <button type="button" onClick={() => onBackendOnly("Changing loop mode persists settings and is disabled here.")}>Change mode</button>
          </div>
        </>
      )}
    />
  );
}

function BundlesDemo({
  options,
  onOptionsChange,
  onBackendOnly,
}: {
  options: Record<string, boolean>;
  onOptionsChange: (options: Record<string, boolean>) => void;
  onBackendOnly: (action: string) => void;
}) {
  return (
    <section className="demo-panel demo-detail">
      <div className="demo-detail-header">
        <div><h2>Bundles</h2><p>Preview import/export transfer bundles.</p></div>
        <div className="demo-actions">
          <button type="button" onClick={() => onBackendOnly("Downloading a real bundle needs backend export data.")}>Download</button>
          <button type="button" onClick={() => onBackendOnly("Applying imports mutates project memory and is disabled here.")}>Apply import</button>
        </div>
      </div>
      <section className="demo-section">
        <h3>Options</h3>
        <div className="demo-check-grid">
          {Object.entries(options).map(([option, checked]) => (
            <label className="demo-check-row" key={option}>
              <input
                checked={checked}
                type="checkbox"
                onChange={(event) => onOptionsChange({ ...options, [option]: event.target.checked })}
              />
              {option}
            </label>
          ))}
        </div>
      </section>
      <MiniCard title="Export preview" text={demoSnapshot.bundles.exportPreview} />
      <MiniCard title="Import preview" text={demoSnapshot.bundles.importPreview} />
    </section>
  );
}

function DemoTwoPane<T>({
  items,
  title,
  subtitle,
  detail,
}: {
  items: T[];
  title: (item: T) => string;
  subtitle: (item: T) => string;
      detail: (item: T) => ReactNode;
}) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  return (
    <DemoIndexedTwoPane
      items={items}
      selectedIndex={selectedIndex}
      onSelect={setSelectedIndex}
      title={title}
      subtitle={subtitle}
      detail={(item) => detail(item)}
    />
  );
}

function DemoIndexedTwoPane<T>({
  items,
  selectedIndex,
  onSelect,
  title,
  subtitle,
  detail,
}: {
  items: T[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  title: (item: T) => string;
  subtitle: (item: T) => string;
  detail: (item: T) => ReactNode;
}) {
  const active = items[selectedIndex] ?? items[0];
  return (
    <div className="demo-panel-grid">
      <section className="demo-panel">
        <div className="demo-list">
          {items.map((item, index) => (
            <button
              key={`${title(item)}-${index}`}
              className={index === selectedIndex ? "demo-list-item demo-selected" : "demo-list-item"}
              type="button"
              onClick={() => onSelect(index)}
            >
              <span><strong>{title(item)}</strong><small>{subtitle(item)}</small></span>
            </button>
          ))}
        </div>
      </section>
      <section className="demo-panel demo-detail">{detail(active)}</section>
    </div>
  );
}

function MetricGrid({ metrics }: { metrics: Array<[string, string | number]> }) {
  return (
    <div className="demo-metric-grid">
      {metrics.map(([label, value]) => (
        <div className="demo-metric" key={label}>
          <span>{label}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </div>
  );
}

function MiniCard({ title, text }: { title: string; text: string }) {
  return (
    <section className="demo-section">
      <h3>{title}</h3>
      <p>{text}</p>
    </section>
  );
}

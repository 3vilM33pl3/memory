import { Metric } from "../../components/Details";
import type { EmbeddingBackendInfo, EmbeddingBackendsResponse } from "../../types";

interface EmbeddingsTabProps {
  embeddingBackends: EmbeddingBackendsResponse | null;
  selectedEmbeddingBackend: EmbeddingBackendInfo | null;
  selectedEmbeddingIndex: number;
  embeddingBusy: boolean;
  embeddingLoading: boolean;
  embeddingOperation: string | null;
  onRefresh: () => void;
  onReindexAll: () => void;
  onReembedAll: () => void;
  onSelectBackend: (index: number) => void;
  onToggleSearch: (backend: EmbeddingBackendInfo) => void;
  onToggleCreation: (backend: EmbeddingBackendInfo) => void;
  onReembedBackend: (backend: EmbeddingBackendInfo) => void;
  onReindexBackend: (backend: EmbeddingBackendInfo) => void;
}

export function EmbeddingsTab({
  embeddingBackends,
  selectedEmbeddingBackend,
  selectedEmbeddingIndex,
  embeddingBusy,
  embeddingLoading,
  embeddingOperation,
  onRefresh,
  onReindexAll,
  onReembedAll,
  onSelectBackend,
  onToggleSearch,
  onToggleCreation,
  onReembedBackend,
  onReindexBackend,
}: EmbeddingsTabProps) {
  return (
    <section className="panel-stack">
      <div className="panel actions-row">
        <button onClick={onRefresh} type="button" disabled={embeddingBusy}>
          {embeddingLoading ? "Refreshing..." : "Refresh"}
        </button>
        <button onClick={onReindexAll} type="button" disabled={embeddingBusy}>Reindex all</button>
        <button onClick={onReembedAll} type="button" disabled={embeddingBusy}>Re-embed all</button>
      </div>
      <section className="panel-grid">
        <div className="panel">
          <h2>Embedding backends</h2>
          <div className="stats-row">
            <span>active {embeddingBackends?.active ?? "none"}</span>
            <span>create {selectedEmbeddingBackend ? `${selectedEmbeddingBackend.create_enabled ? "on" : "off"} for ${selectedEmbeddingBackend.name}` : "unknown"}</span>
            <span>{embeddingBackends?.backends.length ?? 0} configured</span>
            <span>{embeddingBackends?.backends.filter((backend) => backend.ready).length ?? 0} ready</span>
            <span>{embeddingBackends?.backends.filter((backend) => !backend.ready).length ?? 0} not ready</span>
          </div>
          <p className="muted">
            Status: {embeddingOperation ? `${embeddingOperation}...` : embeddingLoading ? "refreshing..." : "idle"}
          </p>
          <div className="list-view">
            {(embeddingBackends?.backends ?? []).map((backend, index) => (
              <button
                key={backend.name}
                type="button"
                className={`list-item ${selectedEmbeddingIndex === index ? "selected" : ""}`}
                onClick={() => onSelectBackend(index)}
              >
                <div>
                  <strong>{backend.active ? "* " : ""}{backend.name}</strong>
                  <p>{backend.provider} · {backend.model}{backend.base_url ? ` · ${backend.base_url}` : ""}</p>
                </div>
                <div className="meta-stack">
                  <span className={`badge ${backend.ready ? "badge-active" : "badge-archived"}`}>{backend.ready ? "ready" : "not ready"}</span>
                  <span className={`badge ${backend.create_enabled ? "badge-active" : "badge-archived"}`}>create {backend.create_enabled ? "on" : "off"}</span>
                  <span>{backend.project_chunk_count ?? 0} chunks</span>
                  <span>{backend.project_memory_count ?? 0} memories</span>
                </div>
              </button>
            ))}
          </div>
        </div>
        <div className="panel detail-scroll">
          {selectedEmbeddingBackend ? (
            <>
              <h2>{selectedEmbeddingBackend.name}</h2>
              <Metric label="Provider" value={selectedEmbeddingBackend.provider} />
              <Metric label="Model" value={selectedEmbeddingBackend.model || "n/a"} />
              <Metric label="Base URL" value={selectedEmbeddingBackend.base_url || "default"} />
              <Metric label="Coverage" value={`${selectedEmbeddingBackend.project_chunk_count ?? 0} chunks / ${selectedEmbeddingBackend.project_memory_count ?? 0} memories`} />
              <Metric label="Status" value={selectedEmbeddingBackend.ready ? "ready" : "not ready"} />
              <Metric label="Search" value={selectedEmbeddingBackend.active ? "active" : "inactive"} />
              <Metric label="Automatic creation" value={selectedEmbeddingBackend.create_enabled ? "on" : "off"} />
              <div className="proposal-actions">
                <button
                  onClick={() => onToggleSearch(selectedEmbeddingBackend)}
                  type="button"
                  disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                >
                  {selectedEmbeddingBackend.active ? "Turn off search" : "Activate"}
                </button>
                <button
                  onClick={() => onToggleCreation(selectedEmbeddingBackend)}
                  type="button"
                  disabled={embeddingBusy}
                >
                  {selectedEmbeddingBackend.create_enabled ? "Disable automatic creation" : "Enable automatic creation"}
                </button>
                <button
                  onClick={() => onReembedBackend(selectedEmbeddingBackend)}
                  type="button"
                  disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                >
                  Create embeddings
                </button>
                <button
                  onClick={() => onReindexBackend(selectedEmbeddingBackend)}
                  type="button"
                  disabled={embeddingBusy || !selectedEmbeddingBackend.ready}
                >
                  Reindex
                </button>
              </div>
              <p className="muted">Shortcuts: Enter toggles search, c toggles automatic creation, e creates embeddings, I reindexes, r refreshes.</p>
            </>
          ) : (
            <p className="muted">No embedding backends configured.</p>
          )}
        </div>
      </section>
    </section>
  );
}

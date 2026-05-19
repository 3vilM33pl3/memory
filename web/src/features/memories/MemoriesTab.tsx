import type { RefObject } from "react";

import { RichText } from "../../components/RichText";
import type { MemoryEntryResponse, MemoryHistoryResponse, MemoryType, ProjectMemoryListItem } from "../../types";
import { formatDateTime } from "../../utils/format";
import type { MemoryTypeFilter, StatusFilter } from "./types";

export const MEMORY_TYPES: MemoryType[] = [
  "architecture",
  "convention",
  "decision",
  "incident",
  "debugging",
  "environment",
  "domain_fact",
  "documentation",
  "task",
  "plan",
  "implementation",
  "user",
  "feedback",
  "project",
  "reference",
];

interface MemoriesTabProps {
  searchRef: RefObject<HTMLInputElement | null>;
  filteredMemories: ProjectMemoryListItem[];
  selectedMemoryId: string | null;
  selectedMemory: MemoryEntryResponse | null;
  selectedHistory: MemoryHistoryResponse | null;
  textFilter: string;
  tagFilter: string;
  statusFilter: StatusFilter;
  typeFilter: MemoryTypeFilter;
  onTextFilterChange: (value: string) => void;
  onTagFilterChange: (value: string) => void;
  onStatusFilterChange: (value: StatusFilter) => void;
  onTypeFilterChange: (value: MemoryTypeFilter) => void;
  onSelectMemory: (memoryId: string) => void;
  onClearHistory: () => void;
  onLoadHistory: (memoryId: string) => void;
  onDelete: (memoryId: string) => void;
}

export function MemoriesTab({
  searchRef,
  filteredMemories,
  selectedMemoryId,
  selectedMemory,
  selectedHistory,
  textFilter,
  tagFilter,
  statusFilter,
  typeFilter,
  onTextFilterChange,
  onTagFilterChange,
  onStatusFilterChange,
  onTypeFilterChange,
  onSelectMemory,
  onClearHistory,
  onLoadHistory,
  onDelete,
}: MemoriesTabProps) {
  return (
    <section className="panel-grid">
      <div className="panel">
        <div className="panel-toolbar filters-grid">
          <input ref={searchRef} placeholder="Search summary or preview (/)" value={textFilter} onChange={(e) => onTextFilterChange(e.target.value)} />
          <input placeholder="Filter tag" value={tagFilter} onChange={(e) => onTagFilterChange(e.target.value)} />
          <select value={statusFilter} onChange={(e) => onStatusFilterChange(e.target.value as StatusFilter)}>
            <option value="all">All statuses</option>
            <option value="active">Active</option>
            <option value="archived">Archived</option>
          </select>
          <select value={typeFilter} onChange={(e) => onTypeFilterChange(e.target.value as MemoryTypeFilter)}>
            <option value="all">All types</option>
            {MEMORY_TYPES.map((memoryType) => (
              <option key={memoryType} value={memoryType}>{memoryType}</option>
            ))}
          </select>
        </div>
        <div className="list-view">
          {filteredMemories.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`list-item ${selectedMemoryId === item.id ? "selected" : ""}`}
              onClick={() => onSelectMemory(item.id)}
            >
              <div>
                <strong>{item.summary}</strong>
                <p>{item.preview}</p>
              </div>
              <div className="meta-stack">
                <span className="badge">{item.memory_type}</span>
                <span className={`badge badge-${item.status}`}>{item.status}</span>
                <span>{item.confidence.toFixed(2)}</span>
              </div>
            </button>
          ))}
        </div>
      </div>
      <div className="panel detail-scroll">
        {selectedHistory ? (
          <>
            <div className="detail-header">
              <div>
                <h2>Version history</h2>
                <p>{selectedHistory.project} · canonical {selectedHistory.canonical_id} · {selectedHistory.versions.length} version(s)</p>
              </div>
              <button onClick={onClearHistory} type="button">Hide history</button>
            </div>
            {selectedHistory.versions.map((version) => (
              <section className="detail-section version-card" key={version.id}>
                <h3>v{version.version_no} {version.is_tombstone ? "(tombstone)" : ""}</h3>
                <p>{version.memory_type} · {version.status} · {formatDateTime(version.updated_at)}</p>
                <strong>{version.summary}</strong>
                {version.is_tombstone ? <p>Memory was deleted at this version.</p> : <RichText text={version.canonical_text} />}
              </section>
            ))}
          </>
        ) : selectedMemory ? (
          <>
            <div className="detail-header">
              <div>
                <h2>{selectedMemory.summary}</h2>
                <p>{selectedMemory.memory_type} · {selectedMemory.status} · confidence {selectedMemory.confidence.toFixed(2)} · importance {selectedMemory.importance} · v{selectedMemory.version_no}</p>
              </div>
              <div className="proposal-actions">
                <button onClick={() => onLoadHistory(selectedMemory.id)} type="button">History</button>
                <button className="danger" onClick={() => onDelete(selectedMemory.id)} type="button">Delete</button>
              </div>
            </div>
            <section className="detail-section">
              <h3>Embeddings</h3>
              {selectedMemory.embedding_spaces.length ? (
                selectedMemory.embedding_spaces.map((space) => (
                  <div key={`${space.provider}-${space.model}-${space.base_url}`} className="metric-row">
                    <span>{space.provider} / {space.model}</span>
                    <strong>{space.chunk_count} chunk(s){space.last_updated ? ` · ${formatDateTime(space.last_updated)}` : ""}</strong>
                  </div>
                ))
              ) : (
                <p className="muted">No embeddings for this memory yet. Run Re-embed for this project to populate the active embedding space.</p>
              )}
            </section>
            <section className="detail-section">
              <h3>Canonical text</h3>
              <RichText text={selectedMemory.canonical_text} />
            </section>
            <section className="detail-section">
              <h3>Tags</h3>
              <div className="tag-wrap">{selectedMemory.tags.map((t) => <span key={t} className="tag">{t}</span>)}</div>
            </section>
            <section className="detail-section">
              <h3>Sources</h3>
              {selectedMemory.sources.map((source) => (
                <div key={source.id} className="source-card">
                  <strong>{source.source_kind}</strong>
                  <p>{source.file_path ?? source.git_commit ?? "<no path>"}</p>
                  {source.excerpt ? <pre>{source.excerpt}</pre> : null}
                </div>
              ))}
            </section>
            <section className="detail-section">
              <h3>Related memories</h3>
              {selectedMemory.related_memories.length ? (
                selectedMemory.related_memories.map((related) => (
                  <div key={`${related.relation_type}-${related.memory_id}`} className="relation-row">
                    <span className="badge">{related.relation_type}</span>
                    <span>{related.summary}</span>
                  </div>
                ))
              ) : (
                <p className="muted">No related memories recorded.</p>
              )}
            </section>
          </>
        ) : (
          <p className="muted">Select a memory to inspect its details.</p>
        )}
      </div>
    </section>
  );
}

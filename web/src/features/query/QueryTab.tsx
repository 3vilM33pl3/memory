import type { FormEvent, RefObject } from "react";

import { RichText } from "../../components/RichText";
import type { MemoryEntryResponse, QueryResponse, QueryResult } from "../../types";
import { formatCitationNumbers, formatNumber, formatTokens } from "../../utils/format";

interface QueryTabProps {
  queryRef: RefObject<HTMLInputElement | null>;
  queryText: string;
  queryResponse: QueryResponse | null;
  activeQueryResult: QueryResult | null;
  selectedQueryMemory: MemoryEntryResponse | null;
  selectedQueryIndex: number;
  selectedQueryMemoryLoading: boolean;
  selectedQueryMemoryError: string | null;
  queryLoading: boolean;
  queryError: string | null;
  queryRoundtripMs: number | null;
  onQueryTextChange: (value: string) => void;
  onSubmit: (event: FormEvent) => void;
  onApplyHistory: (delta: number) => void;
  onResetHistoryCursor: () => void;
  onSelectResult: (index: number) => void;
  onDelete: (memoryId: string) => void;
}

export function QueryTab({
  queryRef,
  queryText,
  queryResponse,
  activeQueryResult,
  selectedQueryMemory,
  selectedQueryIndex,
  selectedQueryMemoryLoading,
  selectedQueryMemoryError,
  queryLoading,
  queryError,
  queryRoundtripMs,
  onQueryTextChange,
  onSubmit,
  onApplyHistory,
  onResetHistoryCursor,
  onSelectResult,
  onDelete,
}: QueryTabProps) {
  return (
    <section className="panel-stack">
      <form className="panel" onSubmit={onSubmit}>
        <div className="panel-toolbar">
          <input
            ref={queryRef}
            className="query-input"
            placeholder="Ask what the project knows... (?)"
            value={queryText}
            onChange={(event) => onQueryTextChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "ArrowUp") {
                event.preventDefault();
                onApplyHistory(-1);
              } else if (event.key === "ArrowDown") {
                event.preventDefault();
                onApplyHistory(1);
              } else {
                onResetHistoryCursor();
              }
            }}
          />
          <button type="submit" disabled={queryLoading}>{queryLoading ? "Searching..." : "Query"}</button>
        </div>
        {queryLoading ? (
          <div className="query-summary">
            <p>Searching "{queryText.trim()}"...</p>
            <p className="muted">Previous results remain visible until the new search finishes.</p>
          </div>
        ) : null}
        {queryError ? (
          <div className="query-summary warning-list">
            <strong>Query failed</strong>
            <p>{queryError}</p>
          </div>
        ) : null}
        {queryResponse ? (
          <div className="query-summary">
            <p>{queryResponse.answer}</p>
            <div className="stats-row">
              <span>{queryResponse.answer_generation.method}</span>
              <span>citations {formatCitationNumbers(queryResponse.answer_generation.cited_result_numbers)}</span>
              <span>answer {queryResponse.answer_generation.duration_ms} ms</span>
              <span>roundtrip {queryRoundtripMs ?? "n/a"} ms</span>
              <span>confidence {queryResponse.confidence.toFixed(2)}</span>
              <span>{queryResponse.insufficient_evidence ? "insufficient evidence" : "sufficient evidence"}</span>
              <span>mode {queryResponse.diagnostics.retrieval_mode}</span>
              <span>lexical {queryResponse.diagnostics.lexical_candidates} / {queryResponse.diagnostics.lexical_duration_ms} ms</span>
              <span>semantic {queryResponse.diagnostics.semantic_candidates} / {queryResponse.diagnostics.semantic_duration_ms} ms [{queryResponse.diagnostics.semantic_status}]</span>
              <span>graph {queryResponse.diagnostics.graph_candidates} / {queryResponse.diagnostics.graph_duration_ms} ms [{queryResponse.diagnostics.graph_status}]</span>
              <span>merged {queryResponse.diagnostics.merged_candidates}</span>
              <span>returned {queryResponse.diagnostics.returned_results}</span>
              <span>relation {queryResponse.diagnostics.relation_augmented_candidates}</span>
              <span>graph augmented {queryResponse.diagnostics.graph_augmented_candidates}</span>
              <span>rerank {queryResponse.diagnostics.rerank_duration_ms} ms</span>
              <span>total {queryResponse.diagnostics.total_duration_ms} ms</span>
              <span>{queryResponse.answer_generation.token_usage ? `${formatTokens(queryResponse.answer_generation.token_usage.total_tokens)} answer tokens` : "tokens n/a"}</span>
            </div>
            {queryResponse.answer_generation.fallback_reason ? (
              <p className="muted">Fallback: {queryResponse.answer_generation.fallback_reason}</p>
            ) : null}
          </div>
        ) : (
          <p className="muted">Run a query to inspect the returned memories and diagnostics.</p>
        )}
      </form>
      <section className="panel-grid">
        <div className="panel">
          <div className="list-view">
            {(queryResponse?.results ?? []).map((result, index) => (
              <button
                key={result.memory_id}
                type="button"
                className={`list-item ${selectedQueryIndex === index ? "selected" : ""}`}
                onClick={() => onSelectResult(index)}
              >
                <div>
                  <strong>{result.summary}</strong>
                  <p>{result.snippet}</p>
                </div>
                <div className="meta-stack">
                  <span className="badge">#{index + 1}</span>
                  <span className="badge">{result.memory_type}</span>
                  <span className="badge">{result.match_kind}</span>
                  {queryResponse?.answer_generation.cited_result_numbers.includes(index + 1) ? <span className="badge badge-active">cited</span> : null}
                  <span>{result.score.toFixed(2)}</span>
                </div>
              </button>
            ))}
          </div>
        </div>
        <div className="panel detail-scroll">
          {activeQueryResult ? (
            <>
              <div className="detail-header">
                <div>
                  <h2>{activeQueryResult.summary}</h2>
                  <p>{activeQueryResult.memory_type} · {activeQueryResult.match_kind} · score {activeQueryResult.score.toFixed(2)}</p>
                </div>
                <button className="danger" onClick={() => onDelete(activeQueryResult.memory_id)} type="button">Delete</button>
              </div>
              <section className="detail-section">
                <h3>Snippet</h3>
                <p>{activeQueryResult.snippet}</p>
              </section>
              <section className="detail-section">
                <h3>Why it ranked</h3>
                <ul>
                  {activeQueryResult.score_explanation.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                <div className="stats-row">
                  <span>chunk fts {formatNumber(activeQueryResult.debug.chunk_fts)}</span>
                  <span>entry fts {formatNumber(activeQueryResult.debug.entry_fts)}</span>
                  <span>semantic {formatNumber(activeQueryResult.debug.semantic_similarity)}</span>
                  <span>relation {formatNumber(activeQueryResult.debug.relation_boost)}</span>
                  <span>overlap {Math.round((activeQueryResult.debug.term_overlap ?? 0) * 100)}%</span>
                  <span>phrases {activeQueryResult.debug.exact_phrase_matches}</span>
                  <span>tags {activeQueryResult.debug.tag_match_count}</span>
                  <span>paths {activeQueryResult.debug.path_match_count}</span>
                  <span>graph {formatNumber(activeQueryResult.debug.graph_boost)}</span>
                  <span>graph matches {activeQueryResult.debug.graph_match_count}</span>
                  <span>graph edges {activeQueryResult.debug.graph_edge_count}</span>
                  <span>importance {activeQueryResult.debug.importance}</span>
                  <span>memory confidence {formatNumber(activeQueryResult.debug.memory_confidence)}</span>
                  <span>recency {formatNumber(activeQueryResult.debug.recency_boost)}</span>
                </div>
              </section>
              {activeQueryResult.graph_connections.length ? (
                <section className="detail-section">
                  <h3>Graph connections</h3>
                  {activeQueryResult.graph_connections.map((connection, index) => (
                    <div key={`${connection.file_path}-${connection.symbol ?? ""}-${connection.neighbor_symbol ?? ""}-${index}`} className="relation-row">
                      <span className="badge">+{connection.score_boost.toFixed(2)}</span>
                      <span>{connection.reason}</span>
                      <span className="muted">
                        {connection.file_path}
                        {connection.symbol ? ` · ${connection.symbol}` : ""}
                        {connection.edge_kind ? ` · ${connection.edge_kind}` : ""}
                        {connection.neighbor_symbol ? ` -> ${connection.neighbor_symbol}` : ""}
                      </span>
                    </div>
                  ))}
                </section>
              ) : null}
              <section className="detail-section">
                <h3>Tags</h3>
                {activeQueryResult.tags.length ? (
                  <div className="tag-wrap">{activeQueryResult.tags.map((tag) => <span key={tag} className="tag">{tag}</span>)}</div>
                ) : (
                  <p className="muted">No tags on this result.</p>
                )}
              </section>
              {activeQueryResult.sources.length ? (
                <section className="detail-section">
                  <h3>Sources</h3>
                  {activeQueryResult.sources.map((source, index) => (
                    <div key={`${source.source_kind}-${source.file_path ?? source.git_commit ?? index}`} className="source-card">
                      <strong>{source.source_kind}</strong>
                      <p>{source.file_path ?? source.git_commit ?? "<no path>"}</p>
                      {source.excerpt ? <pre>{source.excerpt}</pre> : null}
                    </div>
                  ))}
                </section>
              ) : null}
              {selectedQueryMemoryLoading ? <p className="muted">Loading selected memory detail...</p> : null}
              {selectedQueryMemoryError ? <p className="warning-list">Detail unavailable: {selectedQueryMemoryError}</p> : null}
              {selectedQueryMemory ? (
                <>
                  <section className="detail-section">
                    <h3>Memory detail</h3>
                    <RichText text={selectedQueryMemory.canonical_text} />
                  </section>
                  <section className="detail-section">
                    <h3>Related memories</h3>
                    {selectedQueryMemory.related_memories.length ? (
                      selectedQueryMemory.related_memories.map((related) => (
                        <div key={`${related.relation_type}-${related.memory_id}`} className="relation-row">
                          <span className="badge">{related.relation_type}</span>
                          <span>{related.summary}</span>
                          <span className="muted">{related.memory_type} · {related.confidence.toFixed(2)}</span>
                        </div>
                      ))
                    ) : (
                      <p className="muted">No related memories recorded.</p>
                    )}
                  </section>
                </>
              ) : null}
            </>
          ) : (
            <p className="muted">Select a returned memory to inspect its ranking details.</p>
          )}
        </div>
      </section>
    </section>
  );
}

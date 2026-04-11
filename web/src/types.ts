export type MemoryStatus = "active" | "archived";

export type MemoryType =
  | "architecture"
  | "convention"
  | "decision"
  | "incident"
  | "debugging"
  | "environment"
  | "domain_fact"
  | "plan";

export type QueryMatchKind = "lexical" | "semantic" | "hybrid";

export type SourceKind =
  | "task_prompt"
  | "agent_summary"
  | "user_note"
  | "file"
  | "git_commit"
  | "test"
  | "task_title";

export interface NamedCount {
  name: string;
  count: number;
}

export interface MemoryTypeCount {
  memory_type: MemoryType;
  count: number;
}

export interface SourceKindCount {
  source_kind: SourceKind;
  count: number;
}

export interface AutomationStatus {
  enabled: boolean;
  mode: string;
  dirty_file_count: number;
  last_activity_at: string | null;
  last_persisted_at: string | null;
  last_capture_at: string | null;
  last_curation_at: string | null;
  pending_capture_count: number;
  last_decision: string | null;
}

export interface WatcherPresence {
  watcher_id: string;
  repo_root: string;
  hostname: string;
  pid: number;
  mode: string;
  started_at: string;
  last_heartbeat_at: string;
}

export interface WatcherPresenceSummary {
  active_count: number;
  stale_after_seconds: number;
  last_heartbeat_at: string | null;
  watchers: WatcherPresence[];
}

export interface ProjectOverviewResponse {
  project: string;
  service_status: string;
  database_status: string;
  memory_entries_total: number;
  active_memories: number;
  archived_memories: number;
  high_confidence_memories: number;
  medium_confidence_memories: number;
  low_confidence_memories: number;
  recent_memories_7d: number;
  recent_captures_7d: number;
  raw_captures_total: number;
  uncurated_raw_captures: number;
  tasks_total: number;
  sessions_total: number;
  curation_runs_total: number;
  last_memory_at: string | null;
  last_curation_at: string | null;
  last_capture_at: string | null;
  oldest_uncurated_capture_age_hours: number | null;
  embedding_chunks_total: number;
  fresh_embedding_chunks: number;
  stale_embedding_chunks: number;
  missing_embedding_chunks: number;
  embedding_spaces_total: number;
  active_embedding_provider: string | null;
  active_embedding_model: string | null;
  top_tags: NamedCount[];
  top_files: NamedCount[];
  memory_type_breakdown: MemoryTypeCount[];
  source_kind_breakdown: SourceKindCount[];
  automation: AutomationStatus | null;
  watchers: WatcherPresenceSummary | null;
}

export interface ProjectMemoryListItem {
  id: string;
  summary: string;
  preview: string;
  memory_type: MemoryType;
  confidence: number;
  status: MemoryStatus;
  updated_at: string;
  tags: string[];
}

export interface ProjectMemoriesResponse {
  project: string;
  total: number;
  items: ProjectMemoryListItem[];
}

export interface MemorySourceRecord {
  id: string;
  task_id: string | null;
  file_path: string | null;
  git_commit: string | null;
  source_kind: SourceKind;
  excerpt: string | null;
}

export interface RelatedMemorySummary {
  memory_id: string;
  relation_type: string;
  summary: string;
  memory_type: MemoryType;
  confidence: number;
}

export interface MemoryEntryResponse {
  id: string;
  project: string;
  canonical_text: string;
  summary: string;
  memory_type: MemoryType;
  importance: number;
  confidence: number;
  status: MemoryStatus;
  tags: string[];
  sources: MemorySourceRecord[];
  related_memories: RelatedMemorySummary[];
  created_at: string;
  updated_at: string;
}

export interface QueryDiagnostics {
  lexical_candidates: number;
  semantic_candidates: number;
  merged_candidates: number;
  returned_results: number;
  relation_augmented_candidates: number;
  lexical_duration_ms: number;
  semantic_duration_ms: number;
  rerank_duration_ms: number;
  total_duration_ms: number;
  semantic_status: string;
}

export interface ReembedResponse {
  reembedded_chunks: number;
}

export interface QueryResult {
  memory_id: string;
  summary: string;
  snippet: string;
  memory_type: MemoryType;
  score: number;
  match_kind: QueryMatchKind;
  score_explanation: string[];
  debug: {
    chunk_fts: number;
    entry_fts: number;
    semantic_similarity: number;
    exact_phrase_matches: number;
    term_overlap: number;
    tag_match_count: number;
    path_match_count: number;
    relation_boost: number;
    importance: number;
    memory_confidence: number;
    recency_boost: number;
  };
  tags: string[];
  sources: Partial<MemorySourceRecord>[];
}

export interface QueryResponse {
  answer: string;
  confidence: number;
  insufficient_evidence: boolean;
  results: QueryResult[];
  diagnostics: QueryDiagnostics;
}

export interface QueryRequest {
  project: string;
  query: string;
  filters: Record<string, never>;
  top_k: number;
  min_confidence: number | null;
}

export type ActivityKind =
  | "commit_sync"
  | "query"
  | "query_error"
  | "capture_task"
  | "curate"
  | "reindex"
  | "archive"
  | "delete_memory";

export type ActivityDetails =
  | {
      type: "commit_sync";
      imported_count: number;
      updated_count: number;
      total_received: number;
      newest_commit?: string | null;
      oldest_commit?: string | null;
    }
  | {
      type: "query";
      query: string;
      top_k: number;
      result_count: number;
      confidence: number;
      insufficient_evidence: boolean;
      total_duration_ms: number;
      answer?: string | null;
      error?: string | null;
    }
  | {
      type: "capture_task";
      session_id: string;
      task_id: string;
      raw_capture_id: string;
      idempotency_key: string;
      writer_id: string;
    }
  | {
      type: "curate";
      run_id: string;
      input_count: number;
      output_count: number;
    }
  | {
      type: "reindex";
      reindexed_entries: number;
    }
  | {
      type: "archive";
      archived_count: number;
      max_confidence: number;
      max_importance: number;
    }
  | {
      type: "delete_memory";
      deleted: boolean;
      summary: string;
    };

export interface ActivityEvent {
  project: string;
  kind: ActivityKind;
  summary: string;
  details: ActivityDetails | null;
  recorded_at: string;
}

export interface ReindexResponse {
  project: string;
  reindexed_entries: number;
}

export interface CurateResponse {
  run_id: string;
  input_count: number;
  output_count: number;
}

export interface ArchiveResponse {
  project: string;
  archived_count: number;
  max_confidence: number;
  max_importance: number;
}

export interface DeleteMemoryResponse {
  memory_id: string;
  deleted: boolean;
  summary: string;
}

export interface ProjectMemoryExportOptions {
  include_archived: boolean;
  include_tags: boolean;
  include_relations: boolean;
  include_source_file_paths: boolean;
  include_git_commits: boolean;
  include_source_excerpts: boolean;
}

export interface ProjectMemoryBundlePreview {
  bundle_id: string;
  source_project: string;
  exported_at: string;
  summary_markdown: string;
  memory_count: number;
  relation_count: number;
  warning_count: number;
  warnings: string[];
  options: ProjectMemoryExportOptions;
}

export interface ProjectMemoryImportPreview {
  bundle_id: string;
  bundle_hash: string;
  source_project: string;
  target_project: string;
  exported_at: string;
  summary_markdown: string;
  memory_count: number;
  relation_count: number;
  new_count: number;
  unchanged_count: number;
  replacing_count: number;
  warning_count: number;
  warnings: string[];
  options: ProjectMemoryExportOptions;
}

export interface ProjectMemoryImportResponse {
  target_project: string;
  bundle_id: string;
  bundle_hash: string;
  imported_count: number;
  replaced_count: number;
  skipped_count: number;
  relation_count: number;
}

// --- Agent snapshot types ---

export interface ChildProcessResponse {
  pid: number;
  command: string;
  mem_kb: number;
  port: number | null;
}

export interface SubAgentResponse {
  name: string;
  status: string;
  tokens: number;
}

export interface AgentSessionResponse {
  agent_cli: string;
  pid: number;
  session_id: string;
  cwd: string;
  project_name: string;
  started_at: number;
  status: "working" | "waiting" | "done";
  model: string;
  context_percent: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read: number;
  total_cache_create: number;
  turn_count: number;
  current_tasks: string[];
  mem_mb: number;
  version: string;
  git_branch: string;
  git_added: number;
  git_modified: number;
  token_history: number[];
  subagents: SubAgentResponse[];
  mem_file_count: number;
  mem_line_count: number;
  children: ChildProcessResponse[];
  initial_prompt: string;
  first_assistant_text: string;
}

export interface OrphanPortResponse {
  port: number;
  pid: number;
  command: string;
  project_name: string;
}

export interface AgentSnapshotResponse {
  collected_at: string;
  sessions: AgentSessionResponse[];
  orphan_ports: OrphanPortResponse[];
}

// --- Resume types ---

export interface ResumeCheckpoint {
  project: string;
  repo_root: string;
  marked_at: string;
  note: string | null;
  git_branch: string | null;
  git_head: string | null;
}

export interface ResumeAction {
  title: string;
  rationale: string;
  command_hint: string | null;
}

export interface ResumeResponse {
  project: string;
  generated_at: string;
  checkpoint: ResumeCheckpoint | null;
  briefing: string;
  current_thread: string | null;
  change_summary: string[];
  attention_items: string[];
  primary_next_step: ResumeAction | null;
  secondary_next_steps: ResumeAction[];
  context_items: ProjectMemoryListItem[];
  timeline: ActivityEvent[];
  changed_memories: ProjectMemoryListItem[];
  durable_context: ProjectMemoryListItem[];
  warnings: string[];
  actions: ResumeAction[];
  overview: ProjectOverviewResponse;
}

// --- Replacement proposal types ---

export interface ReplacementProposalRecord {
  id: string;
  project: string;
  target_memory_id: string;
  target_summary: string;
  candidate_summary: string;
  candidate_canonical_text: string;
  candidate_memory_type: MemoryType;
  score: number;
  policy: string;
  reasons: string[];
  created_at: string;
}

export interface ReplacementProposalListResponse {
  project: string;
  proposals: ReplacementProposalRecord[];
}

export interface ReplacementProposalResolutionResponse {
  project: string;
  proposal_id: string;
  status: string;
  policy: string;
  target_memory_id: string;
  target_summary: string;
  candidate_summary: string;
  new_memory_id: string | null;
}

// --- Stream types ---

export type StreamRequest =
  | { type: "health" }
  | { type: "project_overview"; project: string }
  | { type: "project_memories"; project: string }
  | { type: "memory_detail"; memory_id: string }
  | { type: "subscribe_project"; project: string }
  | { type: "subscribe_memory"; memory_id: string }
  | { type: "unsubscribe_memory" }
  | { type: "ping" };

export type StreamResponse =
  | { type: "health"; value: unknown }
  | { type: "project_overview"; value: ProjectOverviewResponse }
  | { type: "project_memories"; value: ProjectMemoriesResponse }
  | { type: "memory_detail"; value: MemoryEntryResponse | null }
  | {
      type: "project_snapshot";
      overview: ProjectOverviewResponse;
      memories: ProjectMemoriesResponse;
    }
  | {
      type: "project_changed";
      overview: ProjectOverviewResponse;
      memories: ProjectMemoriesResponse;
    }
  | { type: "memory_snapshot"; detail: MemoryEntryResponse | null }
  | { type: "memory_changed"; detail: MemoryEntryResponse | null }
  | { type: "activity"; event: ActivityEvent }
  | { type: "ack"; message: string }
  | { type: "pong" }
  | { type: "error"; message: string };

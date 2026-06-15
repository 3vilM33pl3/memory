export type MemoryStatus = "active" | "archived";

export type MemoryType =
  | "architecture"
  | "convention"
  | "decision"
  | "incident"
  | "debugging"
  | "environment"
  | "domain_fact"
  | "documentation"
  | "task"
  | "plan"
  | "implementation"
  | "refactor"
  | "user"
  | "feedback"
  | "project"
  | "reference";

export type QueryMatchKind = "lexical" | "semantic" | "hybrid";

export type SourceKind =
  | "task_prompt"
  | "file"
  | "git_commit"
  | "command_output"
  | "test"
  | "note";

export type ReplacementPolicy = "conservative" | "balanced" | "aggressive";

export type LoopMode =
  | "off"
  | "observe"
  | "suggest_only"
  | "draft_output"
  | "autonomous_safe"
  | "paused"
  | "snoozed";

export type LoopRiskLevel = "low" | "medium" | "high" | "critical";

export type LoopScopeType = "user" | "workspace" | "project" | "repo";

export type LoopRunStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled" | "blocked";

export type LoopApprovalStatus = "pending" | "approved" | "rejected" | "edited";

export interface LoopDefinitionRecord {
  id: string;
  loop_id: string;
  version: number;
  name: string;
  description: string;
  risk_level: LoopRiskLevel;
  default_mode: LoopMode;
  trigger_spec: Record<string, unknown>;
  context_spec: Record<string, unknown>;
  policy_spec: Record<string, unknown>;
  output_spec: Record<string, unknown>;
  created_at: string;
}

export interface LoopDefinitionsResponse {
  definitions: LoopDefinitionRecord[];
}

export interface EffectiveLoopSettings {
  loop_id: string;
  enabled: boolean;
  mode: LoopMode;
  scope_type: LoopScopeType;
  scope_id: string;
  global_kill_switch: boolean;
  blocked_reasons?: string[];
  budgets?: Record<string, unknown> | null;
  approval_overrides?: Record<string, unknown> | null;
  paused_until?: string | null;
  snoozed_until?: string | null;
}

export interface LoopDefinitionResponse {
  definition: LoopDefinitionRecord;
  effective_settings?: EffectiveLoopSettings | null;
}

export interface LoopSettingRecord {
  id: string;
  loop_id: string;
  scope_type: LoopScopeType;
  scope_id: string;
  project?: string | null;
  repo_root?: string | null;
  enabled?: boolean | null;
  mode?: LoopMode | null;
  budgets?: Record<string, unknown> | null;
  approval_overrides?: Record<string, unknown> | null;
  paused_until?: string | null;
  snoozed_until?: string | null;
  updated_by?: string | null;
  reason?: string | null;
  updated_at: string;
}

export interface LoopSettingsUpdateRequest {
  scope_type?: LoopScopeType | null;
  scope_id?: string | null;
  project?: string | null;
  repo_root?: string | null;
  enabled?: boolean | null;
  mode?: LoopMode | null;
  budgets?: Record<string, unknown> | null;
  approval_overrides?: Record<string, unknown> | null;
  paused_until?: string | null;
  snoozed_until?: string | null;
  updated_by?: string | null;
  reason?: string | null;
  explicit_user_approval?: boolean;
}

export interface LoopSettingResponse {
  setting: LoopSettingRecord;
  effective_settings: EffectiveLoopSettings;
}

export interface LoopGlobalStateResponse {
  kill_switch_enabled: boolean;
  updated_by?: string | null;
  reason?: string | null;
  updated_at: string;
}

export interface LoopGlobalStateUpdateRequest {
  kill_switch_enabled: boolean;
  updated_by?: string | null;
  reason?: string | null;
}

export interface LoopRunRequest {
  project?: string | null;
  repo_root?: string | null;
  scope_type?: LoopScopeType | null;
  scope_id?: string | null;
  dry_run?: boolean;
  reason?: string | null;
  trigger_payload?: Record<string, unknown> | null;
}

export interface LoopRunSummary {
  id: string;
  loop_id: string;
  definition_version: number;
  project?: string | null;
  repo_root?: string | null;
  mode: LoopMode;
  status: LoopRunStatus;
  started_at: string;
  finished_at?: string | null;
  output_summary?: string | null;
  trace_count: number;
  blocked_reasons?: string[];
}

export interface LoopTriggerEventRecord {
  id: string;
  source: string;
  event_type: string;
  project?: string | null;
  repo_root?: string | null;
  payload_hash: string;
  dedupe_key?: string | null;
  trust_level: string;
  payload: Record<string, unknown>;
  received_at: string;
}

export interface LoopTraceRecord {
  id: string;
  run_id: string;
  sequence: number;
  trace_type: string;
  title: string;
  payload: Record<string, unknown>;
  redacted: boolean;
  created_at: string;
}

export interface LoopMemoryProposalRecord {
  id: string;
  run_id?: string | null;
  project?: string | null;
  loop_id: string;
  proposal_type: string;
  target_memory_id?: string | null;
  candidate: Record<string, unknown>;
  evidence: unknown;
  confidence: number;
  risk_notes?: string | null;
  status: string;
  created_at: string;
  resolved_at?: string | null;
}

export interface LoopMemoryProposalsResponse {
  total_returned: number;
  proposals: LoopMemoryProposalRecord[];
}

export interface LoopMemoryProposalDecisionResponse {
  proposal: LoopMemoryProposalRecord;
  memory_id?: string | null;
}

export interface LoopContextInstructionRef {
  path: string;
  reason: string;
  estimated_tokens: number;
}

export interface LoopContextSourceRef {
  source_kind: SourceKind;
  file_path?: string | null;
  git_commit?: string | null;
  symbol_name?: string | null;
  provenance_status?: string | null;
}

export interface LoopContextMemory {
  memory_id: string;
  canonical_id: string;
  summary: string;
  preview: string;
  memory_type: MemoryType;
  confidence: number;
  importance: number;
  freshness: string;
  updated_at: string;
  tags: string[];
  source_refs: LoopContextSourceRef[];
  estimated_tokens: number;
  stale: boolean;
  contradictory: boolean;
  inclusion_reason: string;
}

export interface LoopContextExclusion {
  memory_id: string;
  summary: string;
  reason: string;
  estimated_tokens: number;
}

export interface LoopContextPack {
  id: string;
  loop_id: string;
  project: string;
  repo_root?: string | null;
  run_id?: string | null;
  generated_at: string;
  token_budget: number;
  estimated_tokens: number;
  instructions: LoopContextInstructionRef[];
  memories: LoopContextMemory[];
  exclusions: LoopContextExclusion[];
  warnings: string[];
  metadata: unknown;
}

export interface LoopContextPackDiff {
  previous_run_id?: string | null;
  previous_pack_id?: string | null;
  added_memory_ids: string[];
  removed_memory_ids: string[];
  changed_memory_ids: string[];
  token_delta: number;
  warning_delta: string[];
}

export interface LoopRunDetail {
  summary: LoopRunSummary;
  run_reason?: string | null;
  trigger_event?: LoopTriggerEventRecord | null;
  effective_settings: unknown;
  policy_decisions: unknown;
  cost: unknown;
  output: unknown;
  traces: LoopTraceRecord[];
  memory_proposals: LoopMemoryProposalRecord[];
  context_pack?: LoopContextPack | null;
  context_diff?: LoopContextPackDiff | null;
}

export interface LoopRunResponse {
  run: LoopRunDetail;
}

export interface LoopRunsResponse {
  total_returned: number;
  runs: LoopRunSummary[];
}

export interface LoopApprovalRequestRecord {
  id: string;
  run_id?: string | null;
  project?: string | null;
  loop_id: string;
  action_type: string;
  proposed_action: Record<string, unknown>;
  risk_reason: string;
  status: LoopApprovalStatus;
  requester?: string | null;
  reviewer?: string | null;
  decision_reason?: string | null;
  created_at: string;
  resolved_at?: string | null;
}

export interface LoopApprovalsResponse {
  total_returned: number;
  approvals: LoopApprovalRequestRecord[];
}

export interface LoopApprovalDecisionResponse {
  approval: LoopApprovalRequestRecord;
}

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
  repo_root: string;
  dirty_file_count: number | null;
  pending_note_count: number | null;
  last_activity_at: string | null;
  last_persisted_at: string | null;
  last_decision: string | null;
}

export type WatcherHealth = "healthy" | "stale" | "restarting" | "failed";

export interface WatcherPresence {
  watcher_id: string;
  project: string;
  repo_root: string;
  hostname: string;
  host_service_id: string;
  pid: number;
  mode: string;
  managed_by_service: boolean;
  health: WatcherHealth;
  started_at: string;
  last_heartbeat_at: string;
  agent_cli: string | null;
  agent_session_id: string | null;
  agent_pid: number | null;
  agent_started_at: string | null;
  last_restart_attempt_at: string | null;
  restart_attempt_count: number;
}

export interface WatcherPresenceSummary {
  active_count: number;
  unhealthy_count: number;
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
  pending_replacement_proposals: number;
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
  importance: number;
  status: MemoryStatus;
  updated_at: string;
  tags: string[];
  tag_count: number;
  source_count: number;
  canonical_id: string;
  version_no: number;
  is_tombstone: boolean;
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
  symbol_name?: string | null;
  symbol_kind?: string | null;
  source_kind: SourceKind;
  excerpt: string | null;
  provenance?: SourceProvenanceRecord | null;
}

export type SourceProvenanceStatus =
  | "verified"
  | "missing_file"
  | "missing_symbol"
  | "unverifiable"
  | "stale";

export interface SourceProvenanceRecord {
  status: SourceProvenanceStatus;
  checked_at: string;
  reason?: string | null;
  resolved_path?: string | null;
}

export interface SourceProvenanceVerification {
  source_id: string;
  memory_id: string;
  memory_summary: string;
  source_kind: SourceKind;
  file_path?: string | null;
  symbol_name?: string | null;
  symbol_kind?: string | null;
  status: SourceProvenanceStatus;
  reason?: string | null;
  resolved_path?: string | null;
}

export interface ProvenanceVerificationResponse {
  project: string;
  repo_root: string;
  dry_run: boolean;
  checked_at: string;
  checked_count: number;
  verified_count: number;
  missing_file_count: number;
  missing_symbol_count: number;
  unverifiable_count: number;
  stale_count: number;
  stored_count: number;
  warnings: DiagnosticInfo[];
  items: SourceProvenanceVerification[];
}

export interface RelatedMemorySummary {
  memory_id: string;
  relation_type: string;
  summary: string;
  memory_type: MemoryType;
  confidence: number;
}

export interface MemoryEmbeddingSpace {
  provider: string;
  model: string;
  base_url: string;
  chunk_count: number;
  last_updated: string | null;
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
  embedding_spaces: MemoryEmbeddingSpace[];
  created_at: string;
  updated_at: string;
  canonical_id: string;
  version_no: number;
  is_tombstone: boolean;
}

export interface MemoryHistoryResponse {
  canonical_id: string;
  project: string;
  versions: MemoryEntryResponse[];
}

export interface QueryDiagnostics {
  retrieval_mode: string;
  lexical_enabled: boolean;
  semantic_enabled: boolean;
  graph_enabled: boolean;
  relation_boost_enabled: boolean;
  lexical_candidates: number;
  semantic_candidates: number;
  merged_candidates: number;
  returned_results: number;
  relation_augmented_candidates: number;
  graph_candidates: number;
  graph_augmented_candidates: number;
  provenance_decayed_candidates: number;
  provenance_unverified_candidates: number;
  lexical_duration_ms: number;
  semantic_duration_ms: number;
  rerank_duration_ms: number;
  graph_duration_ms: number;
  total_duration_ms: number;
  semantic_status: string;
  graph_status: string;
  provenance_warnings: DiagnosticInfo[];
}

export type QueryAnswerMethod = "deterministic" | "llm" | "fallback";

export interface QueryAnswerGeneration {
  method: QueryAnswerMethod;
  cited_result_numbers: number[];
  evidence_count: number;
  duration_ms: number;
  fallback_reason: string | null;
  token_usage?: TokenUsage | null;
}

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  total_tokens: number;
}

export interface QueryAnswerCitation {
  result_number: number;
  memory_id: string;
  memory_type: MemoryType;
  summary: string;
  snippet: string;
}

export interface ReembedResponse {
  reembedded_chunks: number;
  dry_run: boolean;
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
    graph_boost: number;
    graph_match_count: number;
    graph_edge_count: number;
    importance: number;
    memory_confidence: number;
    recency_boost: number;
  };
  tags: string[];
  sources: Partial<MemorySourceRecord>[];
  graph_connections: QueryGraphConnection[];
}

export interface QueryGraphConnection {
  file_path: string;
  symbol?: string | null;
  symbol_kind?: string | null;
  edge_kind?: string | null;
  neighbor_symbol?: string | null;
  direction?: string | null;
  score_boost: number;
  reason: string;
}

export interface QueryResponse {
  answer: string;
  confidence: number;
  insufficient_evidence: boolean;
  answer_generation: QueryAnswerGeneration;
  answer_citations: QueryAnswerCitation[];
  results: QueryResult[];
  diagnostics: QueryDiagnostics;
}

export interface QueryRequest {
  project: string;
  query: string;
  filters: Record<string, never>;
  top_k: number;
  min_confidence: number | null;
  include_stale?: boolean;
  history?: boolean;
}

export type ActivityKind =
  | "checkpoint"
  | "scan"
  | "plan"
  | "commit_sync"
  | "bundle_export"
  | "bundle_import"
  | "graph_extract"
  | "query"
  | "query_error"
  | "watcher_health"
  | "memory_replacement"
  | "capture_task"
  | "curate"
  | "reindex"
  | "reembed"
  | "archive"
  | "delete_memory"
  | "briefing"
  | "diagnostic"
  | "llm_audit";

export type DiagnosticSeverity = "info" | "warning" | "error";

export interface DiagnosticInfo {
  code: string;
  source: string;
  component: string;
  operation: string;
  severity: DiagnosticSeverity;
  message: string;
  raw_error?: string | null;
  explanation?: string | null;
  fix_hint?: string | null;
  doctor_hint?: string | null;
  command_hint?: string | null;
}

export interface LlmAuditMessage {
  role: string;
  content: string;
  truncated: boolean;
}

export type ActivityDetails =
  | {
      type: "checkpoint";
      repo_root: string;
      marked_at: string;
      note?: string | null;
      git_branch?: string | null;
      git_head?: string | null;
    }
  | {
      type: "plan";
      action: "started" | "synced" | "finish_blocked" | "finish_verified";
      title: string;
      thread_key: string;
      total_items: number;
      completed_items: number;
      remaining_items: string[];
      source_path?: string | null;
      verified_complete: boolean;
    }
  | {
      type: "scan";
      dry_run: boolean;
      candidate_count: number;
      files_considered: number;
      commits_considered: number;
      index_reused: boolean;
      report_path: string;
      capture_id?: string | null;
      curate_run_id?: string | null;
    }
  | {
      type: "graph_extract";
      repo_root: string;
      git_head?: string | null;
      since?: string | null;
      extraction_run_id?: string | null;
      dry_run: boolean;
      reused_existing_run: boolean;
      index_reused: boolean;
      analyzer_version: string;
      strategy_version: string;
      symbol_count: number;
      reference_count: number;
      resolved_reference_count: number;
      unresolved_reference_count: number;
      ambiguous_reference_count: number;
      graph_node_count: number;
      graph_edge_count: number;
      evidence_count: number;
    }
  | {
      type: "commit_sync";
      imported_count: number;
      updated_count: number;
      total_received: number;
      newest_commit?: string | null;
      oldest_commit?: string | null;
    }
  | {
      type: "bundle_transfer";
      bundle_id: string;
      item_count: number;
      source_project?: string | null;
    }
  | {
      type: "query";
      query: string;
      top_k: number;
      result_count: number;
      confidence: number;
      insufficient_evidence: boolean;
      total_duration_ms: number;
      graph_status?: string | null;
      graph_candidates: number;
      graph_augmented_candidates: number;
      graph_duration_ms: number;
      graph_result_count: number;
      graph_connection_count: number;
      graph_connections: QueryGraphConnection[];
      answer?: string | null;
      error?: string | null;
    }
  | {
      type: "llm_audit";
      operation: string;
      request_summary: string;
      status: string;
      redacted: boolean;
      truncated: boolean;
      messages: LlmAuditMessage[];
      error?: string | null;
    }
  | {
      type: "watcher_health";
      watcher_id: string;
      hostname: string;
      health: WatcherHealth;
      managed_by_service: boolean;
      restart_attempt_count: number;
      agent_cli?: string | null;
      agent_session_id?: string | null;
      agent_pid?: number | null;
      previous_health?: WatcherHealth | null;
      recovered_after_restart_attempts?: number | null;
      message?: string | null;
    }
  | {
      type: "memory_replacement";
      old_memory_id: string;
      old_summary: string;
      new_memory_id: string;
      new_summary: string;
      automatic: boolean;
      policy: ReplacementPolicy;
    }
  | {
      type: "capture_task";
      session_id: string;
      task_id: string;
      raw_capture_id: string;
      idempotency_key: string;
      task_title?: string | null;
      writer_id: string;
    }
  | {
      type: "curate";
      run_id: string;
      input_count: number;
      output_count: number;
      replaced_count: number;
      proposal_count: number;
    }
  | {
      type: "reindex";
      reindexed_entries: number;
    }
  | {
      type: "reembed";
      reembedded_chunks: number;
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
    }
  | {
      type: "diagnostic";
      diagnostic: DiagnosticInfo;
    };

export interface ActivityEvent {
  id: string;
  project: string;
  kind: ActivityKind;
  memory_id?: string | null;
  summary: string;
  details: ActivityDetails | null;
  actor_id?: string | null;
  actor_name?: string | null;
  source?: string | null;
  operation_id?: string | null;
  duration_ms?: number | null;
  provider?: string | null;
  model?: string | null;
  token_usage?: TokenUsage | null;
  recorded_at: string;
}

export interface ActivityListResponse {
  project: string;
  total_returned: number;
  items: ActivityEvent[];
}

export interface TokenUsageSummary {
  action_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read_tokens: number;
  total_cache_write_tokens: number;
  total_tokens: number;
}

export interface UpToSpeedRequest {
  project: string;
  include_llm_summary: boolean;
  limit: number;
}

export interface UpToSpeedResponse {
  project: string;
  generated_at: string;
  briefing: string;
  current_focus: string[];
  recent_work: string[];
  blockers: string[];
  next_actions: ResumeAction[];
  useful_memories: ProjectMemoryListItem[];
  recent_activities: ActivityEvent[];
  token_usage: TokenUsageSummary;
  warnings: string[];
}

export interface LlmAuditStatusResponse {
  enabled: boolean;
  redacted: boolean;
  max_message_chars: number;
  max_total_chars: number;
  profile: string;
  config_path?: string | null;
}

export interface RuntimeComponentStatus {
  version: string;
  status: string;
  detail?: string | null;
}

export interface RuntimeManagerStatus {
  version: string;
  state: string;
  mode?: string | null;
  detail?: string | null;
  tracked_sessions: number;
  warning_count: number;
  runtime_mode?: string | null;
  last_reconcile_reason?: string | null;
  event_count: number;
  fallback_scan_count: number;
}

export interface RuntimeWatcherStatus {
  version: string;
  status: string;
  detail?: string | null;
  active_count: number;
  unhealthy_count: number;
  stale_after_seconds: number;
}

export interface RuntimeProvenanceStatus {
  status: string;
  enabled: boolean;
  interval_seconds: number;
  last_started_at?: string | null;
  last_finished_at?: string | null;
  last_project?: string | null;
  checked_count: number;
  stale_count: number;
  error?: string | null;
}

export interface RuntimeSkillStatus {
  bundle_version: string;
  status: string;
  summary: string;
}

export interface RuntimeRestartNotice {
  version: string;
  reason: string;
  marker_path: string;
}

export interface RuntimeStatusResponse {
  generated_at: string;
  project: string;
  profile: string;
  web: RuntimeComponentStatus;
  service: RuntimeComponentStatus;
  manager: RuntimeManagerStatus;
  watchers: RuntimeWatcherStatus;
  provenance: RuntimeProvenanceStatus;
  skills: RuntimeSkillStatus;
  restart_notice?: RuntimeRestartNotice | null;
}

export interface ReindexResponse {
  reindexed_entries: number;
  dry_run: boolean;
}

export interface CurateResponse {
  project_id: string;
  run_id: string;
  input_count: number;
  output_count: number;
  replaced_count: number;
  proposal_count: number;
  dry_run: boolean;
}

export interface ArchiveResponse {
  archived_count: number;
  dry_run: boolean;
}

export interface DeleteMemoryResponse {
  memory_id: string;
  project: string;
  deleted: boolean;
  summary: string;
}

export interface ReplacementPolicyResponse {
  project: string;
  repo_root: string | null;
  replacement_policy: ReplacementPolicy;
  writable: boolean;
}

export interface ReplacementPolicyRequest {
  repo_root: string;
  replacement_policy: ReplacementPolicy;
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

export interface RateLimitResponse {
  source: string;
  five_hour_pct: number | null;
  five_hour_resets_at: number | null;
  seven_day_pct: number | null;
  seven_day_resets_at: number | null;
  updated_at: number | null;
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
  rate_limits: RateLimitResponse[];
}

export interface EmbeddingBackendInfo {
  name: string;
  provider: string;
  base_url: string;
  model: string;
  active: boolean;
  ready: boolean;
  create_enabled: boolean;
  project_chunk_count: number | null;
  project_memory_count: number | null;
}

export interface EmbeddingBackendsResponse {
  backends: EmbeddingBackendInfo[];
  active: string | null;
  create_enabled: boolean;
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

export interface CommitRecord {
  hash: string;
  short_hash: string;
  subject: string;
  body: string;
  author_name: string | null;
  author_email: string | null;
  committed_at: string;
  parent_hashes: string[];
  changed_paths: string[];
  imported_at: string;
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
  commits: CommitRecord[];
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

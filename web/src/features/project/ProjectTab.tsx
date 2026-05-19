import { KeyValueList, Metric } from "../../components/Details";
import type { ActivityEvent, ProjectOverviewResponse, ReplacementPolicyResponse, ReplacementProposalRecord } from "../../types";
import { formatDateTime } from "../../utils/format";

interface ProjectTabProps {
  project: string;
  overview: ProjectOverviewResponse;
  activities: ActivityEvent[];
  proposals: ReplacementProposalRecord[];
  replacementPolicy: ReplacementPolicyResponse | null;
  onRefresh: () => void;
  onProjectAction: (action: "curate" | "reindex" | "reembed" | "archive") => void;
  onOpenActivity: (index: number) => void;
  onApproveProposal: (proposalId: string) => void;
  onRejectProposal: (proposalId: string) => void;
}

export function ProjectTab({
  overview,
  activities,
  proposals,
  replacementPolicy,
  onRefresh,
  onProjectAction,
  onOpenActivity,
  onApproveProposal,
  onRejectProposal,
}: ProjectTabProps) {
  return (
    <section className="panel-stack">
      <div className="panel actions-row">
        <button onClick={onRefresh} type="button">Refresh</button>
        <button onClick={() => onProjectAction("curate")} type="button">Curate</button>
        <button onClick={() => onProjectAction("reindex")} type="button">Reindex</button>
        <button onClick={() => onProjectAction("reembed")} type="button">Re-embed</button>
        <button onClick={() => onProjectAction("archive")} type="button">Archive</button>
      </div>
      <section className="project-grid">
        <div className="panel">
          <h2>Overview</h2>
          <Metric label="Service" value={`${overview.service_status} / ${overview.database_status}`} />
          <Metric label="Memories" value={`${overview.memory_entries_total} total / ${overview.active_memories} active / ${overview.archived_memories} archived`} />
          <Metric label="Confidence bins" value={`${overview.high_confidence_memories} high / ${overview.medium_confidence_memories} medium / ${overview.low_confidence_memories} low`} />
          <Metric label="Recent 7d" value={`${overview.recent_memories_7d} memories / ${overview.recent_captures_7d} captures`} />
          <Metric label="Raw captures" value={`${overview.raw_captures_total} total / ${overview.uncurated_raw_captures} uncurated`} />
          <Metric label="Embeddings" value={`${overview.embedding_chunks_total} chunks / ${overview.fresh_embedding_chunks} active-space / ${overview.stale_embedding_chunks} other-space only / ${overview.missing_embedding_chunks} missing active-space`} />
          <Metric label="Embedding spaces" value={`${overview.embedding_spaces_total} stored space(s)`} />
          <Metric label="Active embedding" value={overview.active_embedding_model ? `${overview.active_embedding_provider} / ${overview.active_embedding_model}` : "disabled"} />
          <Metric label="Curation policy" value={`${replacementPolicy?.replacement_policy ?? "unknown"} / ${overview.pending_replacement_proposals} pending (Review tab)`} />
          <Metric label="Tasks / Sessions / Runs" value={`${overview.tasks_total} / ${overview.sessions_total} / ${overview.curation_runs_total}`} />
          <Metric label="Last memory" value={formatDateTime(overview.last_memory_at)} />
          <Metric label="Last curation" value={formatDateTime(overview.last_curation_at)} />
          <Metric label="Last capture" value={formatDateTime(overview.last_capture_at)} />
          <Metric
            label="Automation"
            value={
              overview.automation
                ? `${overview.automation.mode} · dirty ${overview.automation.dirty_file_count ?? 0} · notes ${overview.automation.pending_note_count ?? 0} · ${overview.automation.repo_root}`
                : "not configured"
            }
          />
          <Metric label="Watchers" value={`${overview.watchers?.active_count ?? 0} healthy / ${overview.watchers?.unhealthy_count ?? 0} unhealthy`} />
        </div>
        <div className="panel">
          <h2>Memory types</h2>
          <KeyValueList items={overview.memory_type_breakdown.map((item) => [item.memory_type, String(item.count)])} empty="No memory type data." />
          <h2 style={{ marginTop: "1rem" }}>Source kinds</h2>
          <KeyValueList items={overview.source_kind_breakdown.map((item) => [item.source_kind, String(item.count)])} empty="No source kind data." />
        </div>
        <div className="panel">
          <h2>Top tags</h2>
          <KeyValueList items={overview.top_tags.map((item) => [item.name, String(item.count)])} empty="No tags yet." />
        </div>
        <div className="panel">
          <h2>Top files</h2>
          <KeyValueList items={overview.top_files.map((item) => [item.name, String(item.count)])} empty="No file provenance yet." />
        </div>
        <div className="panel">
          <h2>Recent activity</h2>
          {activities.length ? (
            activities.slice(0, 6).map((event, index) => (
              <button
                key={`${event.recorded_at}-${event.kind}-${index}`}
                type="button"
                className="activity-row-button"
                onClick={() => onOpenActivity(index)}
              >
                <span className="muted">{formatDateTime(event.recorded_at)}</span>
                <strong>{event.kind}</strong>
                <span>{event.summary}</span>
              </button>
            ))
          ) : (
            <p className="muted">No recent activity in this browser session.</p>
          )}
        </div>
      </section>
      {proposals.length > 0 && (
        <div className="panel">
          <h2>Replacement proposals ({proposals.length})</h2>
          {proposals.map((proposal) => (
            <div key={proposal.id} className="proposal-card">
              <p><strong>Target:</strong> {proposal.target_summary}</p>
              <p><strong>Candidate:</strong> {proposal.candidate_summary}</p>
              <p className="muted">
                {proposal.candidate_memory_type} · score {proposal.score} · {proposal.policy}
                {proposal.reasons.length > 0 && ` · ${proposal.reasons.join(", ")}`}
              </p>
              <div className="proposal-actions">
                <button className="approve-btn" onClick={() => onApproveProposal(proposal.id)} type="button">Approve</button>
                <button className="reject-btn" onClick={() => onRejectProposal(proposal.id)} type="button">Reject</button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

import { RichText } from "../../components/RichText";
import type { ReplacementPolicyResponse, ReplacementProposalRecord } from "../../types";

interface ReviewTabProps {
  effectiveRepoRoot: string;
  proposals: ReplacementProposalRecord[];
  activeProposal: ReplacementProposalRecord | null;
  selectedProposalIndex: number;
  replacementPolicy: ReplacementPolicyResponse | null;
  onRefresh: () => void;
  onCyclePolicy: () => void;
  onSelectProposal: (index: number) => void;
  onApproveProposal: (proposalId: string) => void;
  onRejectProposal: (proposalId: string) => void;
}

export function ReviewTab({
  effectiveRepoRoot,
  proposals,
  activeProposal,
  selectedProposalIndex,
  replacementPolicy,
  onRefresh,
  onCyclePolicy,
  onSelectProposal,
  onApproveProposal,
  onRejectProposal,
}: ReviewTabProps) {
  return (
    <section className="panel-grid">
      <div className="panel detail-scroll">
        <div className="detail-header">
          <div>
            <h2>Curation review</h2>
            <p>
              Policy {replacementPolicy?.replacement_policy ?? "unknown"} · {proposals.length} pending
              {proposals.length ? ` · selected ${selectedProposalIndex + 1}/${proposals.length}` : ""}
              {effectiveRepoRoot ? ` · ${effectiveRepoRoot}` : ""}
            </p>
          </div>
          <div className="proposal-actions">
            <button onClick={onRefresh} type="button">Refresh</button>
            <button onClick={onCyclePolicy} type="button" disabled={!effectiveRepoRoot}>Cycle policy</button>
          </div>
        </div>
        {!effectiveRepoRoot ? (
          <p className="warning-list">Set a repo root to change policy. Approve/reject still works for queued proposals.</p>
        ) : null}
        <div className="list-view">
          {proposals.length ? proposals.map((proposal, index) => (
            <button
              key={proposal.id}
              type="button"
              aria-label={`Select proposal ${proposal.target_summary}`}
              className={`list-item ${activeProposal?.id === proposal.id ? "selected" : ""}`}
              onClick={() => onSelectProposal(index)}
            >
              <div>
                <strong>{proposal.target_summary}</strong>
                <p>{proposal.candidate_summary}</p>
              </div>
              <div className="meta-stack">
                <span className="badge">{proposal.candidate_memory_type}</span>
                <span>{proposal.score}</span>
              </div>
            </button>
          )) : <p className="muted">No pending replacement proposals.</p>}
        </div>
      </div>
      <div className="panel detail-scroll">
        {activeProposal ? (
          <>
            <h2>Proposal detail</h2>
            <section className="detail-section">
              <h3>Target</h3>
              <p>{activeProposal.target_summary}</p>
            </section>
            <section className="detail-section">
              <h3>Candidate</h3>
              <p><strong>{activeProposal.candidate_summary}</strong></p>
              <RichText text={activeProposal.candidate_canonical_text} />
            </section>
            <div className="stats-row">
              <span className="badge">{activeProposal.candidate_memory_type}</span>
              <span className="badge">{activeProposal.policy}</span>
              <span>score {activeProposal.score}</span>
              {activeProposal.reasons.map((reason) => <span key={reason}>{reason}</span>)}
            </div>
            <div className="proposal-actions">
              <button className="approve-btn" onClick={() => onApproveProposal(activeProposal.id)} type="button">Approve</button>
              <button className="reject-btn" onClick={() => onRejectProposal(activeProposal.id)} type="button">Reject</button>
            </div>
          </>
        ) : (
          <p className="muted">Queued ambiguous curation candidates will appear here.</p>
        )}
      </div>
    </section>
  );
}

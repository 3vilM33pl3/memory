import { RichText } from "../../components/RichText";
import type { ResumeResponse } from "../../types";
import { formatDateTime } from "../../utils/format";

interface ResumeTabProps {
  resumeData: ResumeResponse | null;
  resumeLoading: boolean;
  onLoadResume: () => void;
}

export function ResumeTab({ resumeData, resumeLoading, onLoadResume }: ResumeTabProps) {
  return (
    <section className="panel-stack">
      <div className="panel actions-row">
        <button onClick={onLoadResume} type="button" disabled={resumeLoading}>
          {resumeLoading ? "Generating..." : "Load resume"}
        </button>
      </div>
      <div className="panel detail-scroll">
        {resumeLoading ? (
          <p className="loading-indicator">Generating project briefing...</p>
        ) : resumeData ? (
          <>
            <h2>Resume for {resumeData.project}</h2>
            <p className="muted">Generated {formatDateTime(resumeData.generated_at)}</p>

            {resumeData.checkpoint && (
              <div className="resume-section">
                <h3>Checkpoint</h3>
                <p>{resumeData.checkpoint.note ?? "Checkpoint saved"}</p>
                <p className="muted">
                  {formatDateTime(resumeData.checkpoint.marked_at)}
                  {resumeData.checkpoint.git_branch ? ` · ${resumeData.checkpoint.git_branch}` : ""}
                  {resumeData.checkpoint.git_head ? ` · ${resumeData.checkpoint.git_head.slice(0, 8)}` : ""}
                </p>
              </div>
            )}

            {resumeData.current_thread && (
              <div className="resume-section">
                <h3>Current work</h3>
                <p>{resumeData.current_thread}</p>
              </div>
            )}

            {resumeData.primary_next_step && (
              <div className="resume-section">
                <h3>Next step</h3>
                <div className="action-card">
                  <strong>{resumeData.primary_next_step.title}</strong>
                  <p>{resumeData.primary_next_step.rationale}</p>
                  {resumeData.primary_next_step.command_hint && <code>{resumeData.primary_next_step.command_hint}</code>}
                </div>
              </div>
            )}

            {resumeData.secondary_next_steps.length > 0 && (
              <div className="resume-section">
                <h3>Other actions</h3>
                {resumeData.secondary_next_steps.map((action) => (
                  <div key={action.title} className="action-card">
                    <strong>{action.title}</strong>
                    <p>{action.rationale}</p>
                    {action.command_hint && <code>{action.command_hint}</code>}
                  </div>
                ))}
              </div>
            )}

            {resumeData.change_summary.length > 0 && (
              <div className="resume-section">
                <h3>What changed</h3>
                <ul>{resumeData.change_summary.map((item) => <li key={item}>{item}</li>)}</ul>
              </div>
            )}

            {resumeData.attention_items.length > 0 && (
              <div className="resume-section">
                <h3>Needs attention</h3>
                <ul>{resumeData.attention_items.map((item) => <li key={item}>{item}</li>)}</ul>
              </div>
            )}

            {resumeData.context_items.length > 0 && (
              <div className="resume-section">
                <h3>Keep in mind</h3>
                {resumeData.context_items.map((mem) => (
                  <div key={mem.id} className="metric-row">
                    <span className="badge">{mem.memory_type}</span>
                    <span>{mem.summary}</span>
                  </div>
                ))}
              </div>
            )}

            {resumeData.durable_context.length > 0 && (
              <div className="resume-section">
                <h3>Durable context</h3>
                {resumeData.durable_context.map((mem) => (
                  <div key={mem.id} className="metric-row">
                    <span className="badge">{mem.memory_type}</span>
                    <span>{mem.summary}</span>
                  </div>
                ))}
              </div>
            )}

            {resumeData.timeline.length > 0 && (
              <div className="resume-section">
                <h3>Timeline</h3>
                {resumeData.timeline.map((event, i) => (
                  <div key={`${event.recorded_at}-${i}`} className="metric-row">
                    <span className="muted">{formatDateTime(event.recorded_at)}</span>
                    <span>{event.summary}</span>
                  </div>
                ))}
              </div>
            )}

            {resumeData.warnings.length > 0 && (
              <div className="resume-section">
                <h3>Warnings</h3>
                <ul className="warning-list">{resumeData.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
              </div>
            )}

            {resumeData.actions.length > 0 && (
              <div className="resume-section">
                <h3>All suggested next actions</h3>
                {resumeData.actions.map((action) => (
                  <div key={`${action.title}-${action.rationale}`} className="action-card">
                    <strong>{action.title}</strong>
                    <p>{action.rationale}</p>
                    {action.command_hint && <code>{action.command_hint}</code>}
                  </div>
                ))}
              </div>
            )}

            {resumeData.commits.length > 0 && (
              <div className="resume-section">
                <h3>Recent commits</h3>
                {resumeData.commits.map((commit) => (
                  <div key={commit.hash} className="metric-row">
                    <span className="badge">{commit.short_hash}</span>
                    <span>{commit.subject}</span>
                    <span className="muted">{formatDateTime(commit.committed_at)}</span>
                  </div>
                ))}
              </div>
            )}

            <div className="resume-section">
              <h3>Briefing</h3>
              <RichText text={resumeData.briefing} />
            </div>
          </>
        ) : (
          <p className="muted">Click "Load resume" to generate a project briefing with next steps and context.</p>
        )}
      </div>
    </section>
  );
}

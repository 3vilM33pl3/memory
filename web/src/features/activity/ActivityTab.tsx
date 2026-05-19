import { RichText } from "../../components/RichText";
import type { ActivityEvent, LlmAuditStatusResponse, UpToSpeedResponse } from "../../types";
import { activityDurationLabel, activityTokenLabel, formatDateTime, formatTokens } from "../../utils/format";
import { ActivityDetail } from "./ActivityDetail";

interface ActivityTabProps {
  activities: ActivityEvent[];
  activeActivity: ActivityEvent | null;
  selectedActivityIndex: number;
  upToSpeed: UpToSpeedResponse | null;
  upToSpeedLoading: boolean;
  upToSpeedError: string | null;
  llmAudit: LlmAuditStatusResponse | null;
  llmAuditLoading: boolean;
  llmAuditError: string | null;
  llmAuditToggling: boolean;
  onLoadUpToSpeed: (includeLlmSummary: boolean) => void;
  onToggleLlmAudit: () => void;
  onSelectActivity: (index: number) => void;
}

export function ActivityTab({
  activities,
  activeActivity,
  selectedActivityIndex,
  upToSpeed,
  upToSpeedLoading,
  upToSpeedError,
  llmAudit,
  llmAuditLoading,
  llmAuditError,
  llmAuditToggling,
  onLoadUpToSpeed,
  onToggleLlmAudit,
  onSelectActivity,
}: ActivityTabProps) {
  return (
    <section className="panel-stack">
      <div className="panel activity-briefing">
        <div className="detail-header">
          <div>
            <h2>Get Up To Speed</h2>
            <p className="muted">
              Uses persisted activities, recent memory changes, commits, warnings, and token summaries.
            </p>
          </div>
          <div className="proposal-actions">
            <button onClick={() => onLoadUpToSpeed(false)} type="button" disabled={upToSpeedLoading}>
              Deterministic
            </button>
            <button onClick={() => onLoadUpToSpeed(true)} type="button" disabled={upToSpeedLoading}>
              LLM briefing
            </button>
            <button onClick={onToggleLlmAudit} type="button" disabled={llmAuditToggling || llmAuditLoading}>
              {llmAudit?.enabled ? "Disable LLM audit" : "Enable LLM audit"}
            </button>
          </div>
        </div>
        {upToSpeedLoading ? <p className="loading-indicator">Generating get-up-to-speed briefing...</p> : null}
        {upToSpeedError ? <p className="warning-list">Briefing failed: {upToSpeedError}</p> : null}
        {upToSpeed ? (
          <>
            <RichText text={upToSpeed.briefing} />
            <div className="stats-row">
              <span>{upToSpeed.recent_activities.length} activities</span>
              <span>{upToSpeed.useful_memories.length} useful memories</span>
              <span>{upToSpeed.token_usage.action_count} token-tracked actions</span>
              <span>{formatTokens(upToSpeed.token_usage.total_tokens)} tokens</span>
            </div>
          </>
        ) : (
          <p className="muted">Generate a deterministic briefing for a cheap handoff, or an LLM briefing for a synthesized narrative.</p>
        )}
        <p className="muted">
          LLM audit: {llmAuditToggling ? "updating" : llmAuditLoading ? "loading" : llmAudit ? `${llmAudit.enabled ? "on" : "off"} · redaction ${llmAudit.redacted ? "on" : "off"} · ${llmAudit.profile}` : "unknown"}
          {llmAuditError ? ` · ${llmAuditError}` : ""}
        </p>
      </div>
      <section className="panel-grid">
        <div className="panel">
          <div className="list-view">
            {activities.map((event, index) => (
              <button
                key={`${event.recorded_at}-${event.kind}-${index}`}
                type="button"
                className={`list-item ${selectedActivityIndex === index ? "selected" : ""}`}
                onClick={() => onSelectActivity(index)}
              >
                <div>
                  <strong>{event.kind}</strong>
                  <p>{event.summary}</p>
                  <p className="muted">{activityTokenLabel(event)} · {activityDurationLabel(event)}</p>
                </div>
                <span>{formatDateTime(event.recorded_at)}</span>
              </button>
            ))}
          </div>
        </div>
        <div className="panel detail-scroll">
          {activeActivity ? (
            <>
              <h2>{activeActivity.kind}</h2>
              <p>{activeActivity.summary}</p>
              <p className="muted">
                {formatDateTime(activeActivity.recorded_at)} · {activityTokenLabel(activeActivity)} · {activityDurationLabel(activeActivity)}
              </p>
              <ActivityDetail event={activeActivity} />
            </>
          ) : (
            <p className="muted">Keep this page open while queries, captures, curation runs, and deletions happen.</p>
          )}
        </div>
      </section>
    </section>
  );
}

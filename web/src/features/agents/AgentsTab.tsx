import { Metric } from "../../components/Details";
import type { AgentSessionResponse, AgentSnapshotResponse } from "../../types";
import { formatDateTime, formatElapsed, formatEpochSeconds, formatPercent, formatTokens } from "../../utils/format";

interface AgentsTabProps {
  agentSnapshot: AgentSnapshotResponse | null;
  sessions: AgentSessionResponse[];
  selectedAgent: AgentSessionResponse | null;
  selectedAgentIndex: number;
  onSelectAgent: (index: number) => void;
}

export function AgentsTab({ agentSnapshot, sessions, selectedAgent, selectedAgentIndex, onSelectAgent }: AgentsTabProps) {
  return (
    <section className="panel-grid">
      <div className="panel">
        <div className="list-view">
          {sessions.length ? (
            sessions.map((session, index) => (
              <button
                key={session.session_id}
                type="button"
                className={`list-item ${selectedAgentIndex === index ? "selected" : ""}`}
                onClick={() => onSelectAgent(index)}
              >
                <div>
                  <strong>{session.project_name}</strong>
                  <p>{session.current_tasks.join(", ") || "idle"}</p>
                </div>
                <div className="meta-stack">
                  <span className="badge">{session.agent_cli}</span>
                  <span className={`status-pill status-${session.status}`}>{session.status}</span>
                  <span>{session.model}</span>
                  <span>{formatElapsed(session.started_at)}</span>
                </div>
              </button>
            ))
          ) : (
            <p className="muted">No agent sessions detected.</p>
          )}
        </div>
      </div>
      <div className="panel detail-scroll">
        {selectedAgent ? (
          <>
            <h2>{selectedAgent.project_name}</h2>
            <p className={`status-pill status-${selectedAgent.status}`}>{selectedAgent.status}</p>
            <Metric label="Collected" value={formatDateTime(agentSnapshot?.collected_at)} />
            <Metric label="Agent" value={`${selectedAgent.agent_cli} ${selectedAgent.version}`} />
            <Metric label="Session" value={selectedAgent.session_id} />
            <Metric label="PID" value={String(selectedAgent.pid)} />
            <Metric label="Model" value={selectedAgent.model} />
            <Metric label="Context" value={`${selectedAgent.context_percent.toFixed(1)}%`} />
            <Metric label="Turns" value={String(selectedAgent.turn_count)} />
            <Metric label="Tokens" value={`${formatTokens(selectedAgent.total_input_tokens)} in / ${formatTokens(selectedAgent.total_output_tokens)} out`} />
            <Metric label="Cache" value={`${formatTokens(selectedAgent.total_cache_read)} read / ${formatTokens(selectedAgent.total_cache_create)} create`} />
            <Metric label="Memory" value={`${selectedAgent.mem_mb} MB`} />
            <Metric label="Working directory" value={selectedAgent.cwd} />
            <Metric label="Git" value={`${selectedAgent.git_branch || "n/a"} (+${selectedAgent.git_added} ~${selectedAgent.git_modified})`} />
            <Metric label="Prompt" value={selectedAgent.initial_prompt || "n/a"} />
            <Metric label="Current tasks" value={selectedAgent.current_tasks.join(", ") || "none"} />
            {selectedAgent.subagents.length > 0 && (
              <section className="detail-section">
                <h3>Subagents</h3>
                {selectedAgent.subagents.map((sa) => (
                  <div key={sa.name} className="metric-row">
                    <span>{sa.name} ({sa.status})</span>
                    <strong>{formatTokens(sa.tokens)} tokens</strong>
                  </div>
                ))}
              </section>
            )}
            {selectedAgent.children.length > 0 && (
              <section className="detail-section">
                <h3>Child processes</h3>
                {selectedAgent.children.map((ch) => (
                  <div key={ch.pid} className="metric-row">
                    <span>PID {ch.pid}: {ch.command}</span>
                    <strong>{ch.port ? `port ${ch.port}` : `${ch.mem_kb} KB`}</strong>
                  </div>
                ))}
              </section>
            )}
            {(agentSnapshot?.orphan_ports.length ?? 0) > 0 && (
              <section className="detail-section">
                <h3>Orphan ports</h3>
                {agentSnapshot!.orphan_ports.map((op) => (
                  <div key={`${op.pid}-${op.port}`} className="metric-row">
                    <span>:{op.port} (PID {op.pid}) {op.command}</span>
                    <strong>{op.project_name}</strong>
                  </div>
                ))}
              </section>
            )}
            {(agentSnapshot?.rate_limits.length ?? 0) > 0 && (
              <section className="detail-section">
                <h3>Rate limits</h3>
                {agentSnapshot!.rate_limits.map((limit) => (
                  <div key={limit.source} className="metric-row">
                    <span>{limit.source}</span>
                    <strong>
                      5h {formatPercent(limit.five_hour_pct)} / 7d {formatPercent(limit.seven_day_pct)}
                    </strong>
                    <span className="muted">
                      resets {formatEpochSeconds(limit.five_hour_resets_at)} / {formatEpochSeconds(limit.seven_day_resets_at)}
                    </span>
                  </div>
                ))}
              </section>
            )}
          </>
        ) : (
          <p className="muted">Select an agent session to inspect its details.</p>
        )}
      </div>
    </section>
  );
}

import { Metric } from "../../components/Details";
import type {
  LoopApprovalRequestRecord,
  LoopGlobalStateResponse,
  LoopMemoryProposalRecord,
  LoopMode,
  LoopRunDetail,
} from "../../types";
import { formatDateTime } from "../../utils/format";
import type { AutomationCardState } from "./useAutomationsController";

const MODE_OPTIONS: LoopMode[] = [
  "observe",
  "suggest_only",
  "draft_output",
  "autonomous_safe",
];

interface AutomationsTabProps {
  automations: AutomationCardState[];
  activeAutomation: AutomationCardState | null;
  selectedAutomationIndex: number;
  automationsLoading: boolean;
  automationBusy: boolean;
  automationOperation: string | null;
  loopGlobalState: LoopGlobalStateResponse | null;
  selectedLoopRun: LoopRunDetail | null;
  selectedLoopRunApprovals: LoopApprovalRequestRecord[];
  selectedLoopRunLoading: boolean;
  approvalQueue: LoopApprovalRequestRecord[];
  approvalEdits: Record<string, string>;
  proposalEdits: Record<string, string>;
  onRefresh: () => void;
  onSelectAutomation: (index: number) => void;
  onSetLoopMode: (loopId: string, mode: LoopMode) => void;
  onDisableLoop: (loopId: string) => void;
  onPauseLoop: (loopId: string) => void;
  onSnoozeLoop: (loopId: string) => void;
  onRunLoop: (loopId: string) => void;
  onLoadLoopRun: (runId: string) => void;
  onApprovalEditChange: (approvalId: string, value: string) => void;
  onApprovalDecision: (
    approval: LoopApprovalRequestRecord,
    action: "approve" | "reject" | "edit",
  ) => void;
  onProposalEditChange: (proposalId: string, value: string) => void;
  onProposalDecision: (
    proposal: LoopMemoryProposalRecord,
    action: "approve" | "reject" | "edit",
  ) => void;
  onToggleGlobalKillSwitch: () => void;
}

function label(value: string): string {
  return value.replaceAll("_", " ");
}

function stringList(spec: Record<string, unknown>, key: string): string[] {
  const value = spec[key];
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

function joinedList(spec: Record<string, unknown>, key: string): string {
  const values = stringList(spec, key);
  return values.length ? values.map(label).join(", ") : "n/a";
}

function scopeLabel(card: AutomationCardState): string {
  const settings = card.effectiveSettings;
  if (!settings) return "definition default";
  if (settings.scope_type === "repo") return "repo override";
  if (settings.scope_type === "project") return "project override";
  if (settings.scope_type === "workspace") return "workspace override";
  return "inherited user default";
}

function dailyBudgetLabel(card: AutomationCardState): string {
  const budgets = card.effectiveSettings?.budgets;
  if (!budgets) return "inherited";
  const dailyRuns = budgets.daily_runs ?? budgets.daily_run_limit ?? budgets.runs_per_day;
  const dailyCost = budgets.daily_cost ?? budgets.daily_cost_limit;
  if (typeof dailyRuns === "number" || typeof dailyRuns === "string") return `${dailyRuns} runs/day`;
  if (typeof dailyCost === "number" || typeof dailyCost === "string") return `${dailyCost} cost/day`;
  return "custom";
}

function nextTriggerLabel(card: AutomationCardState): string {
  const triggers = stringList(card.definition.trigger_spec, "supported");
  const next = triggers.find((trigger) => trigger !== "manual") ?? triggers[0];
  return next ? label(next) : "manual";
}

function modeLabel(mode: LoopMode): string {
  return label(mode);
}

function jsonPreview(value: unknown): string {
  if (value === null || value === undefined) return "n/a";
  return JSON.stringify(value, null, 2);
}

function proposalIdFromApproval(approval: LoopApprovalRequestRecord): string | null {
  const proposedAction = approval.proposed_action;
  const proposalId = proposedAction.proposal_id ?? proposedAction.memory_proposal_id;
  return typeof proposalId === "string" ? proposalId : null;
}

function proposalForApproval(
  approval: LoopApprovalRequestRecord,
  proposals: LoopMemoryProposalRecord[],
): LoopMemoryProposalRecord | null {
  const proposalId = proposalIdFromApproval(approval);
  if (!proposalId) return null;
  return proposals.find((proposal) => proposal.id === proposalId) ?? null;
}

function policyDecisions(value: unknown): Array<Record<string, unknown>> {
  return Array.isArray(value)
    ? value.filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
    : [];
}

function traceCount(run: LoopRunDetail | null, type: string): number {
  return run?.traces.filter((trace) => trace.trace_type === type).length ?? 0;
}

function automationStateLabel(card: AutomationCardState, globalKillSwitch: boolean): string {
  const settings = card.effectiveSettings;
  if (globalKillSwitch || settings?.global_kill_switch) return "globally stopped";
  if (!settings?.enabled) return "disabled";
  if ((settings.blocked_reasons ?? []).length) return "blocked";
  return modeLabel(settings.mode);
}

function riskBadgeClass(risk: string): string {
  if (risk === "low") return "badge badge-active";
  if (risk === "critical" || risk === "high") return "badge badge-archived";
  return "badge status-connecting";
}

function statusBadgeClass(card: AutomationCardState, globalKillSwitch: boolean): string {
  const state = automationStateLabel(card, globalKillSwitch);
  return state === "disabled" || state === "blocked" || state === "globally stopped"
    ? "badge badge-archived"
    : "badge badge-active";
}

export function AutomationsTab({
  automations,
  activeAutomation,
  selectedAutomationIndex,
  automationsLoading,
  automationBusy,
  automationOperation,
  loopGlobalState,
  selectedLoopRun,
  selectedLoopRunApprovals,
  selectedLoopRunLoading,
  approvalQueue,
  approvalEdits,
  proposalEdits,
  onRefresh,
  onSelectAutomation,
  onSetLoopMode,
  onDisableLoop,
  onPauseLoop,
  onSnoozeLoop,
  onRunLoop,
  onLoadLoopRun,
  onApprovalEditChange,
  onApprovalDecision,
  onProposalEditChange,
  onProposalDecision,
  onToggleGlobalKillSwitch,
}: AutomationsTabProps) {
  const globalKillSwitch = loopGlobalState?.kill_switch_enabled ?? false;
  const activeLoopRun = selectedLoopRun?.summary.loop_id === activeAutomation?.definition.loop_id
    ? selectedLoopRun
    : null;
  const decisions = policyDecisions(activeLoopRun?.policy_decisions);
  const renderApproval = (
    approval: LoopApprovalRequestRecord,
    proposals: LoopMemoryProposalRecord[],
  ) => {
    const proposal = proposalForApproval(approval, proposals);
    const editValue = approvalEdits[approval.id] ?? jsonPreview(approval.proposed_action);
    return (
      <div className="trace-card" key={approval.id}>
        <div className="detail-header">
          <strong>{label(approval.action_type)} · {approval.status}</strong>
          <span className={approval.status === "pending" ? "badge status-connecting" : "badge"}>
            {approval.created_at ? formatDateTime(approval.created_at) : "n/a"}
          </span>
        </div>
        <p>{approval.risk_reason}</p>
        <div className="stats-row">
          <span>loop {approval.loop_id}</span>
          <span>run {approval.run_id ?? "n/a"}</span>
          <span>requester {approval.requester ?? "n/a"}</span>
          <span>reviewer {approval.reviewer ?? "n/a"}</span>
        </div>
        {approval.decision_reason ? <p className="muted">Decision: {approval.decision_reason}</p> : null}
        {proposal ? (
          <div className="action-card">
            <strong>Memory proposal {label(proposal.proposal_type)}</strong>
            <p>
              status {proposal.status} · confidence {proposal.confidence.toFixed(2)}
              {proposal.risk_notes ? ` · ${proposal.risk_notes}` : ""}
            </p>
            {proposal.target_memory_id ? <p>Target: {proposal.target_memory_id}</p> : null}
            <pre className="json-preview">{jsonPreview({ candidate: proposal.candidate, evidence: proposal.evidence })}</pre>
          </div>
        ) : null}
        <label className="approval-edit-label" htmlFor={`approval-edit-${approval.id}`}>Proposed action</label>
        <textarea
          id={`approval-edit-${approval.id}`}
          className="approval-edit"
          value={editValue}
          disabled={approval.status !== "pending" || automationBusy}
          onChange={(event) => onApprovalEditChange(approval.id, event.target.value)}
          rows={6}
        />
        {approval.status === "pending" ? (
          <div className="proposal-actions">
            <button
              className="approve-btn"
              onClick={() => onApprovalDecision(approval, "approve")}
              type="button"
              disabled={automationBusy}
            >
              Approve
            </button>
            <button
              onClick={() => onApprovalDecision(approval, "edit")}
              type="button"
              disabled={automationBusy}
            >
              Save edit
            </button>
            <button
              className="reject-btn"
              onClick={() => onApprovalDecision(approval, "reject")}
              type="button"
              disabled={automationBusy}
            >
              Reject
            </button>
          </div>
        ) : null}
      </div>
    );
  };
  const renderMemoryProposal = (proposal: LoopMemoryProposalRecord) => {
    const editValue = proposalEdits[proposal.id] ?? jsonPreview(proposal.candidate);
    return (
      <div className="trace-card" key={proposal.id}>
        <div className="detail-header">
          <strong>{label(proposal.proposal_type)} · {proposal.status}</strong>
          <span className={proposal.status === "pending" ? "badge status-connecting" : "badge"}>
            {formatDateTime(proposal.created_at)}
          </span>
        </div>
        <p>confidence {proposal.confidence.toFixed(2)}{proposal.risk_notes ? ` · ${proposal.risk_notes}` : ""}</p>
        {proposal.target_memory_id ? <p>Target: {proposal.target_memory_id}</p> : null}
        <label className="approval-edit-label" htmlFor={`proposal-edit-${proposal.id}`}>Candidate</label>
        <textarea
          id={`proposal-edit-${proposal.id}`}
          className="approval-edit"
          value={editValue}
          disabled={proposal.status !== "pending" || automationBusy}
          onChange={(event) => onProposalEditChange(proposal.id, event.target.value)}
          rows={6}
        />
        <pre className="json-preview">{jsonPreview({ evidence: proposal.evidence })}</pre>
        {proposal.status === "pending" ? (
          <div className="proposal-actions">
            <button
              className="approve-btn"
              onClick={() => onProposalDecision(proposal, "approve")}
              type="button"
              disabled={automationBusy}
            >
              Approve
            </button>
            <button
              onClick={() => onProposalDecision(proposal, "edit")}
              type="button"
              disabled={automationBusy}
            >
              Save edit
            </button>
            <button
              className="reject-btn"
              onClick={() => onProposalDecision(proposal, "reject")}
              type="button"
              disabled={automationBusy}
            >
              Reject
            </button>
          </div>
        ) : null}
      </div>
    );
  };
  return (
    <section className="panel-stack">
      <div className="panel actions-row">
        <button onClick={onRefresh} type="button" disabled={automationBusy}>
          {automationsLoading ? "Refreshing..." : "Refresh"}
        </button>
        <button
          className={globalKillSwitch ? "reject-btn" : ""}
          onClick={onToggleGlobalKillSwitch}
          type="button"
          disabled={automationBusy}
        >
          {globalKillSwitch ? "Disable global stop" : "Global stop"}
        </button>
        <span className={`badge ${globalKillSwitch ? "badge-archived" : "badge-active"}`}>
          global {globalKillSwitch ? "stopped" : "ready"}
        </span>
        <span className="muted">
          {automationOperation ? `${automationOperation}...` : `${automations.length} configured`}
        </span>
      </div>

      <div className="panel">
        <div className="detail-header">
          <div>
            <h2>Approval queue</h2>
            <p className="muted">Pending risky actions and memory proposals waiting for review.</p>
          </div>
          <span className={approvalQueue.length ? "badge status-connecting" : "badge badge-active"}>
            {approvalQueue.length} pending
          </span>
        </div>
        {approvalQueue.length ? (
          <div className="list-view">
            {approvalQueue.map((approval) => renderApproval(approval, activeLoopRun?.memory_proposals ?? []))}
          </div>
        ) : (
          <p className="muted">No pending loop approvals.</p>
        )}
      </div>

      <section className="panel-grid">
        <div className="panel">
          <h2>Automations</h2>
          <div className="list-view automations-list">
            {automations.map((card, index) => {
              const settings = card.effectiveSettings;
              const currentMode = settings?.mode ?? card.definition.default_mode;
              return (
                <div
                  key={card.definition.loop_id}
                  role="button"
                  tabIndex={0}
                  className={`list-item automation-card ${selectedAutomationIndex === index ? "selected" : ""}`}
                  onClick={() => onSelectAutomation(index)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") onSelectAutomation(index);
                  }}
                >
                  <div>
                    <strong>{card.definition.name}</strong>
                    <p>{card.definition.description}</p>
                    <div className="stats-row">
                      <span className={statusBadgeClass(card, globalKillSwitch)}>
                        {automationStateLabel(card, globalKillSwitch)}
                      </span>
                      <span className={riskBadgeClass(card.definition.risk_level)}>
                        {card.definition.risk_level} risk
                      </span>
                      <span>{scopeLabel(card)}</span>
                      <span>next {nextTriggerLabel(card)}</span>
                    </div>
                  </div>
                  <div className="meta-stack">
                    <span>budget {dailyBudgetLabel(card)}</span>
                    <span>last {card.lastRun ? `${card.lastRun.status} ${formatDateTime(card.lastRun.started_at)}` : "never"}</span>
                    <select
                      aria-label={`Mode for ${card.definition.name}`}
                      value={currentMode === "off" || currentMode === "paused" || currentMode === "snoozed" ? card.definition.default_mode : currentMode}
                      disabled={automationBusy}
                      onClick={(event) => event.stopPropagation()}
                      onChange={(event) => onSetLoopMode(card.definition.loop_id, event.target.value as LoopMode)}
                    >
                      {MODE_OPTIONS.map((mode) => (
                        <option key={mode} value={mode}>{modeLabel(mode)}</option>
                      ))}
                    </select>
                  </div>
                </div>
              );
            })}
            {!automations.length ? <p className="muted">No loop automations are registered.</p> : null}
          </div>
        </div>

        <div className="panel detail-scroll">
          {activeAutomation ? (
            <>
              <div className="detail-header">
                <div>
                  <h2>{activeAutomation.definition.name}</h2>
                  <p className="muted">{activeAutomation.definition.loop_id}</p>
                </div>
                <span className={statusBadgeClass(activeAutomation, globalKillSwitch)}>
                  {automationStateLabel(activeAutomation, globalKillSwitch)}
                </span>
              </div>
              <Metric label="Mode" value={modeLabel(activeAutomation.effectiveSettings?.mode ?? activeAutomation.definition.default_mode)} />
              <Metric label="Scope" value={scopeLabel(activeAutomation)} />
              <Metric label="Risk" value={activeAutomation.definition.risk_level} />
              <Metric label="Next trigger" value={nextTriggerLabel(activeAutomation)} />
              <Metric label="Daily budget" value={dailyBudgetLabel(activeAutomation)} />
              <Metric label="Outputs" value={joinedList(activeAutomation.definition.output_spec, "produces")} />
              <Metric label="Capabilities" value={joinedList(activeAutomation.definition.context_spec, "capabilities")} />
              <Metric label="Paused until" value={formatDateTime(activeAutomation.effectiveSettings?.paused_until)} />
              <Metric label="Snoozed until" value={formatDateTime(activeAutomation.effectiveSettings?.snoozed_until)} />
              <Metric label="Last run" value={activeAutomation.lastRun ? `${activeAutomation.lastRun.status} ${formatDateTime(activeAutomation.lastRun.started_at)}` : "never"} />
              {(activeAutomation.effectiveSettings?.blocked_reasons ?? []).length ? (
                <div className="detail-section">
                  <h3>Blocked reasons</h3>
                  <ul>
                    {(activeAutomation.effectiveSettings?.blocked_reasons ?? []).map((reason) => (
                      <li key={reason}>{label(reason)}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
              {activeAutomation.lastRun?.output_summary ? (
                <div className="detail-section">
                  <h3>Last output</h3>
                  <p>{activeAutomation.lastRun.output_summary}</p>
                </div>
              ) : null}
              {activeAutomation.lastRun ? (
                <div className="detail-section">
                  <div className="detail-header">
                    <h3>Run detail</h3>
                    <button
                      onClick={() => onLoadLoopRun(activeAutomation.lastRun!.id)}
                      type="button"
                      disabled={automationBusy}
                    >
                      {selectedLoopRunLoading ? "Loading..." : "Load run"}
                    </button>
                  </div>
                  {activeLoopRun ? (
                    <div className="run-detail">
                      <div className="stats-row">
                        <span className={activeLoopRun.summary.status === "succeeded" ? "badge badge-active" : "badge badge-archived"}>
                          {activeLoopRun.summary.status}
                        </span>
                        <span>version {activeLoopRun.summary.definition_version}</span>
                        <span>mode {modeLabel(activeLoopRun.summary.mode)}</span>
                        <span>{activeLoopRun.summary.trace_count} traces</span>
                        <span>{traceCount(activeLoopRun, "policy")} policy</span>
                        <span>{traceCount(activeLoopRun, "command")} commands</span>
                        <span>{traceCount(activeLoopRun, "artifact")} artifacts</span>
                      </div>
                      <Metric label="Trigger" value={activeLoopRun.trigger_event ? `${activeLoopRun.trigger_event.source} / ${activeLoopRun.trigger_event.event_type}` : "n/a"} />
                      <Metric label="Trigger trust" value={activeLoopRun.trigger_event?.trust_level ?? "n/a"} />
                      <Metric label="Trigger received" value={formatDateTime(activeLoopRun.trigger_event?.received_at)} />
                      <Metric label="Run reason" value={activeLoopRun.run_reason || "n/a"} />
                      <Metric label="Started" value={formatDateTime(activeLoopRun.summary.started_at)} />
                      <Metric label="Finished" value={formatDateTime(activeLoopRun.summary.finished_at)} />
                      {activeLoopRun.context_pack ? (
                        <div className="detail-section">
                          <h3>Context pack</h3>
                          <div className="stats-row">
                            <span>{activeLoopRun.context_pack.memories.length} memories</span>
                            <span>{activeLoopRun.context_pack.estimated_tokens}/{activeLoopRun.context_pack.token_budget} tokens</span>
                            <span>{activeLoopRun.context_pack.instructions.length} instruction refs</span>
                            <span>{activeLoopRun.context_pack.exclusions.length} exclusions</span>
                          </div>
                          {activeLoopRun.context_diff ? (
                            <div className="stats-row">
                              <span>+{activeLoopRun.context_diff.added_memory_ids.length}</span>
                              <span>-{activeLoopRun.context_diff.removed_memory_ids.length}</span>
                              <span>~{activeLoopRun.context_diff.changed_memory_ids.length}</span>
                              <span>token delta {activeLoopRun.context_diff.token_delta}</span>
                            </div>
                          ) : null}
                          {activeLoopRun.context_pack.memories.slice(0, 5).map((memory) => (
                            <div className="trace-card" key={memory.memory_id}>
                              <strong>{memory.summary}</strong>
                              <div className="stats-row">
                                <span>{label(memory.memory_type)}</span>
                                <span>conf {memory.confidence.toFixed(2)}</span>
                                <span>{memory.freshness}</span>
                                {memory.stale ? <span className="badge badge-archived">stale</span> : null}
                                {memory.contradictory ? <span className="badge badge-archived">contradictory</span> : null}
                              </div>
                            </div>
                          ))}
                          {activeLoopRun.context_pack.warnings.length ? (
                            <ul>
                              {activeLoopRun.context_pack.warnings.map((warning) => (
                                <li key={warning}>{warning}</li>
                              ))}
                            </ul>
                          ) : null}
                        </div>
                      ) : null}
                      {activeLoopRun.summary.status === "failed" || activeLoopRun.summary.status === "blocked" ? (
                        <div className="detail-section">
                          <h3>Diagnostics</h3>
                          <p>{activeLoopRun.summary.output_summary || "No diagnostic summary recorded."}</p>
                          {(activeLoopRun.summary.blocked_reasons ?? []).length ? (
                            <ul>
                              {(activeLoopRun.summary.blocked_reasons ?? []).map((reason) => (
                                <li key={reason}>{label(reason)}</li>
                              ))}
                            </ul>
                          ) : null}
                        </div>
                      ) : null}
                      <div className="detail-section">
                        <h3>Policy gates</h3>
                        {decisions.length ? decisions.map((decision, index) => (
                          <div className="trace-card" key={`${decision.action}-${index}`}>
                            <strong>{label(String(decision.action ?? "unknown action"))}</strong>
                            <div className="stats-row">
                              <span className={decision.allowed ? "badge badge-active" : "badge badge-archived"}>
                                {decision.allowed ? "allowed" : "blocked"}
                              </span>
                              <span>{decision.requires_approval ? "approval required" : "no approval"}</span>
                              <span>{String(decision.reason ?? "no reason")}</span>
                            </div>
                          </div>
                        )) : <p className="muted">No policy decisions recorded.</p>}
                      </div>
                      <div className="detail-section">
                        <h3>Memory proposals</h3>
                        {activeLoopRun.memory_proposals.length
                          ? activeLoopRun.memory_proposals.map((proposal) => renderMemoryProposal(proposal))
                          : <p className="muted">No memory proposals recorded for this run.</p>}
                      </div>
                      <div className="detail-section">
                        <h3>Approvals</h3>
                        {selectedLoopRunApprovals.length
                          ? selectedLoopRunApprovals.map((approval) =>
                            renderApproval(approval, activeLoopRun.memory_proposals),
                          )
                          : <p className="muted">No approvals recorded for this run.</p>}
                      </div>
                      <div className="detail-section">
                        <h3>Cost summary</h3>
                        <pre className="json-preview">{jsonPreview(activeLoopRun.cost)}</pre>
                      </div>
                      <div className="detail-section">
                        <h3>Effective settings</h3>
                        <pre className="json-preview">{jsonPreview(activeLoopRun.effective_settings)}</pre>
                      </div>
                      <div className="detail-section">
                        <h3>Output</h3>
                        <pre className="json-preview">{jsonPreview(activeLoopRun.output)}</pre>
                      </div>
                      <div className="detail-section">
                        <h3>Trace ledger</h3>
                        {activeLoopRun.traces.length ? activeLoopRun.traces.map((trace) => (
                          <div className="trace-card" key={trace.id}>
                            <div className="detail-header">
                              <strong>{trace.sequence}. {trace.title}</strong>
                              <span className="badge">{trace.trace_type}</span>
                            </div>
                            <p>{formatDateTime(trace.created_at)}</p>
                            {trace.redacted ? (
                              <p className="warning-list">Payload redacted.</p>
                            ) : (
                              <pre className="json-preview">{jsonPreview(trace.payload)}</pre>
                            )}
                          </div>
                        )) : <p className="muted">No trace entries recorded.</p>}
                      </div>
                    </div>
                  ) : (
                    <p className="muted">Load the latest run to inspect trigger, policy, traces, proposals, approvals, output, and cost.</p>
                  )}
                </div>
              ) : null}
              <div className="proposal-actions automations-actions">
                <button
                  onClick={() => onSetLoopMode(activeAutomation.definition.loop_id, activeAutomation.definition.default_mode)}
                  type="button"
                  disabled={automationBusy}
                >
                  Enable
                </button>
                <button
                  onClick={() => onDisableLoop(activeAutomation.definition.loop_id)}
                  type="button"
                  disabled={automationBusy}
                >
                  Disable
                </button>
                <button
                  onClick={() => onPauseLoop(activeAutomation.definition.loop_id)}
                  type="button"
                  disabled={automationBusy}
                >
                  Pause 1h
                </button>
                <button
                  onClick={() => onSnoozeLoop(activeAutomation.definition.loop_id)}
                  type="button"
                  disabled={automationBusy}
                >
                  Snooze 1d
                </button>
                <button
                  onClick={() => onRunLoop(activeAutomation.definition.loop_id)}
                  type="button"
                  disabled={automationBusy || globalKillSwitch}
                >
                  Run now
                </button>
              </div>
            </>
          ) : (
            <p className="muted">Select an automation to inspect its settings.</p>
          )}
        </div>
      </section>
    </section>
  );
}

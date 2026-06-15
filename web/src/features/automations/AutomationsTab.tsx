import { Metric } from "../../components/Details";
import type { LoopGlobalStateResponse, LoopMode } from "../../types";
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
  onRefresh: () => void;
  onSelectAutomation: (index: number) => void;
  onSetLoopMode: (loopId: string, mode: LoopMode) => void;
  onDisableLoop: (loopId: string) => void;
  onPauseLoop: (loopId: string) => void;
  onSnoozeLoop: (loopId: string) => void;
  onRunLoop: (loopId: string) => void;
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

function automationStateLabel(card: AutomationCardState, globalKillSwitch: boolean): string {
  const settings = card.effectiveSettings;
  if (globalKillSwitch || settings?.global_kill_switch) return "globally stopped";
  if (!settings?.enabled) return "disabled";
  if (settings.blocked_reasons.length) return "blocked";
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
  onRefresh,
  onSelectAutomation,
  onSetLoopMode,
  onDisableLoop,
  onPauseLoop,
  onSnoozeLoop,
  onRunLoop,
  onToggleGlobalKillSwitch,
}: AutomationsTabProps) {
  const globalKillSwitch = loopGlobalState?.kill_switch_enabled ?? false;
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
              {activeAutomation.effectiveSettings?.blocked_reasons.length ? (
                <div className="detail-section">
                  <h3>Blocked reasons</h3>
                  <ul>
                    {activeAutomation.effectiveSettings.blocked_reasons.map((reason) => (
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

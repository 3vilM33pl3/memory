import { useEffect, useState } from "react";

import {
  disableLoop,
  enableLoop,
  getLoopDefinition,
  getLoopGlobalState,
  getLoopRuns,
  listLoopDefinitions,
  pauseLoop,
  runLoop,
  snoozeLoop,
  updateLoopGlobalState,
} from "../../api";
import type {
  EffectiveLoopSettings,
  LoopDefinitionRecord,
  LoopGlobalStateResponse,
  LoopMode,
  LoopRunSummary,
  LoopScopeType,
  LoopSettingsUpdateRequest,
} from "../../types";

export interface AutomationCardState {
  definition: LoopDefinitionRecord;
  effectiveSettings: EffectiveLoopSettings | null;
  lastRun: LoopRunSummary | null;
}

interface AutomationsControllerOptions {
  activeTab: string;
  project: string;
  effectiveRepoRoot: string;
  setStatusMessage: (message: string) => void;
  refreshProject: (project: string) => Promise<void>;
}

function addHours(hours: number): string {
  const date = new Date();
  date.setHours(date.getHours() + hours);
  return date.toISOString();
}

function addDays(days: number): string {
  const date = new Date();
  date.setDate(date.getDate() + days);
  return date.toISOString();
}

export function useAutomationsController({
  activeTab,
  project,
  effectiveRepoRoot,
  setStatusMessage,
  refreshProject,
}: AutomationsControllerOptions) {
  const [automations, setAutomations] = useState<AutomationCardState[]>([]);
  const [selectedAutomationIndex, setSelectedAutomationIndex] = useState(0);
  const [automationsLoading, setAutomationsLoading] = useState(false);
  const [automationOperation, setAutomationOperation] = useState<string | null>(null);
  const [loopGlobalState, setLoopGlobalState] = useState<LoopGlobalStateResponse | null>(null);

  useEffect(() => {
    if (activeTab !== "automations") return;
    void refreshAutomations();
  }, [activeTab, project, effectiveRepoRoot]);

  function settingsRequest(
    reason: string,
    extra: Partial<LoopSettingsUpdateRequest> = {},
  ): LoopSettingsUpdateRequest {
    const scopeType: LoopScopeType = effectiveRepoRoot ? "repo" : "project";
    return {
      project,
      repo_root: effectiveRepoRoot || null,
      scope_type: scopeType,
      scope_id: effectiveRepoRoot || project,
      updated_by: "web",
      reason,
      explicit_user_approval: true,
      ...extra,
    };
  }

  async function refreshAutomations(quiet = false) {
    setAutomationsLoading(true);
    try {
      const [definitionsPayload, globalPayload] = await Promise.all([
        listLoopDefinitions(),
        getLoopGlobalState(),
      ]);
      const cards = await Promise.all(
        definitionsPayload.definitions.map(async (definition) => {
          const [detail, runs] = await Promise.all([
            getLoopDefinition(definition.loop_id, project, effectiveRepoRoot || null),
            getLoopRuns({ project, loopId: definition.loop_id, limit: 1 }),
          ]);
          return {
            definition: detail.definition,
            effectiveSettings: detail.effective_settings ?? null,
            lastRun: runs.runs[0] ?? null,
          };
        }),
      );
      setLoopGlobalState(globalPayload);
      setAutomations(cards);
      setSelectedAutomationIndex((current) => Math.min(current, Math.max(cards.length - 1, 0)));
      if (!quiet) setStatusMessage(`Loaded ${cards.length} loop automation(s).`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setAutomationsLoading(false);
    }
  }

  async function runAutomationAction(loopId: string, label: string, action: () => Promise<unknown>) {
    setAutomationOperation(label);
    try {
      await action();
      await refreshAutomations(true);
      await refreshProject(project);
      setStatusMessage(`${label} finished for ${loopId}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setAutomationOperation(null);
    }
  }

  async function handleSetLoopMode(loopId: string, mode: LoopMode) {
    await runAutomationAction(loopId, `Set ${loopId} to ${mode}`, () =>
      enableLoop(loopId, settingsRequest(`Set mode to ${mode} from the browser UI.`, { mode })),
    );
  }

  async function handleDisableLoop(loopId: string) {
    await runAutomationAction(loopId, `Disable ${loopId}`, () =>
      disableLoop(loopId, settingsRequest("Disabled from the browser UI.")),
    );
  }

  async function handlePauseLoop(loopId: string) {
    await runAutomationAction(loopId, `Pause ${loopId}`, () =>
      pauseLoop(
        loopId,
        settingsRequest("Paused from the browser UI.", {
          paused_until: addHours(1),
        }),
      ),
    );
  }

  async function handleSnoozeLoop(loopId: string) {
    await runAutomationAction(loopId, `Snooze ${loopId}`, () =>
      snoozeLoop(
        loopId,
        settingsRequest("Snoozed from the browser UI.", {
          snoozed_until: addDays(1),
        }),
      ),
    );
  }

  async function handleRunLoop(loopId: string) {
    await runAutomationAction(loopId, `Run ${loopId}`, () =>
      runLoop(loopId, {
        project,
        repo_root: effectiveRepoRoot || null,
        scope_type: effectiveRepoRoot ? "repo" : "project",
        scope_id: effectiveRepoRoot || project,
        dry_run: true,
        reason: "Manual browser UI run now request.",
        trigger_payload: { source: "web" },
      }),
    );
  }

  async function handleToggleGlobalKillSwitch() {
    const next = !(loopGlobalState?.kill_switch_enabled ?? false);
    setAutomationOperation(next ? "Enable global kill switch" : "Disable global kill switch");
    try {
      const payload = await updateLoopGlobalState({
        kill_switch_enabled: next,
        updated_by: "web",
        reason: next
          ? "Enabled from the browser UI."
          : "Disabled from the browser UI.",
      });
      setLoopGlobalState(payload);
      await refreshAutomations(true);
      setStatusMessage(`Global loop kill switch ${next ? "enabled" : "disabled"}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setAutomationOperation(null);
    }
  }

  return {
    automations,
    activeAutomation: automations[selectedAutomationIndex] ?? null,
    selectedAutomationIndex,
    setSelectedAutomationIndex,
    automationsLoading,
    automationOperation,
    automationBusy: automationsLoading || automationOperation !== null,
    loopGlobalState,
    refreshAutomations,
    handleSetLoopMode,
    handleDisableLoop,
    handlePauseLoop,
    handleSnoozeLoop,
    handleRunLoop,
    handleToggleGlobalKillSwitch,
  };
}

import { useEffect, useState } from "react";

import {
  approveLoopMemoryProposal,
  approveLoopApproval,
  disableLoop,
  enableLoop,
  editLoopMemoryProposal,
  editLoopApproval,
  getLoopApprovals,
  getLoopDefinition,
  getLoopGlobalState,
  getLoopRun,
  getLoopRuns,
  listLoopDefinitions,
  pauseLoop,
  rejectLoopMemoryProposal,
  rejectLoopApproval,
  runLoop,
  snoozeLoop,
  updateLoopGlobalState,
} from "../../api";
import type {
  EffectiveLoopSettings,
  LoopApprovalRequestRecord,
  LoopDefinitionRecord,
  LoopGlobalStateResponse,
  LoopMemoryProposalRecord,
  LoopMode,
  LoopRunDetail,
  LoopRunResponse,
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
  const [selectedLoopRun, setSelectedLoopRun] = useState<LoopRunDetail | null>(null);
  const [selectedLoopRunApprovals, setSelectedLoopRunApprovals] = useState<LoopApprovalRequestRecord[]>([]);
  const [selectedLoopRunLoading, setSelectedLoopRunLoading] = useState(false);
  const [approvalQueue, setApprovalQueue] = useState<LoopApprovalRequestRecord[]>([]);
  const [approvalEdits, setApprovalEdits] = useState<Record<string, string>>({});
  const [proposalEdits, setProposalEdits] = useState<Record<string, string>>({});

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
      const [definitionsPayload, globalPayload, approvalsPayload] = await Promise.all([
        listLoopDefinitions(),
        getLoopGlobalState(),
        getLoopApprovals({ project, status: "pending", limit: 50 }),
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
      setApprovalQueue(approvalsPayload.approvals);
      setAutomations(cards);
      setSelectedAutomationIndex((current) => Math.min(current, Math.max(cards.length - 1, 0)));
      setSelectedLoopRun((current) => {
        if (!current) return null;
        return cards.some((card) => card.lastRun?.id === current.summary.id) ? current : null;
      });
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
      const result = await action();
      await refreshAutomations(true);
      await refreshProject(project);
      setStatusMessage(`${label} finished for ${loopId}.`);
      return result;
    } catch (error) {
      setStatusMessage((error as Error).message);
      return null;
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
    const result = await runAutomationAction(loopId, `Run ${loopId}`, () =>
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
    const response = result as LoopRunResponse | null;
    if (response?.run) {
      setSelectedLoopRun(response.run);
      await loadLoopRunApprovals(response.run.summary.id);
    }
  }

  async function loadLoopRunApprovals(runId: string) {
    try {
      const payload = await getLoopApprovals({ project, runId, limit: 100 });
      setSelectedLoopRunApprovals(payload.approvals);
    } catch (error) {
      setSelectedLoopRunApprovals([]);
      setStatusMessage((error as Error).message);
    }
  }

  async function refreshLoopRunDetail(runId: string) {
    const payload = await getLoopRun(runId);
    setSelectedLoopRun(payload.run);
    await loadLoopRunApprovals(runId);
  }

  async function handleLoadLoopRun(runId: string) {
    setSelectedLoopRunLoading(true);
    try {
      await refreshLoopRunDetail(runId);
      setStatusMessage(`Loaded loop run ${runId}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setSelectedLoopRunLoading(false);
    }
  }

  function setApprovalEdit(approvalId: string, value: string) {
    setApprovalEdits((current) => ({ ...current, [approvalId]: value }));
  }

  function setProposalEdit(proposalId: string, value: string) {
    setProposalEdits((current) => ({ ...current, [proposalId]: value }));
  }

  async function refreshApprovalSurfaces(runId?: string | null) {
    const approvalsPayload = await getLoopApprovals({ project, status: "pending", limit: 50 });
    setApprovalQueue(approvalsPayload.approvals);
    if (runId) {
      await refreshLoopRunDetail(runId);
    } else if (selectedLoopRun) {
      await refreshLoopRunDetail(selectedLoopRun.summary.id);
    }
  }

  async function handleApprovalDecision(
    approval: LoopApprovalRequestRecord,
    action: "approve" | "reject" | "edit",
  ) {
    setAutomationOperation(`${action} ${approval.action_type}`);
    try {
      if (action === "approve") {
        await approveLoopApproval(approval.id);
      } else if (action === "reject") {
        await rejectLoopApproval(approval.id);
      } else {
        const editText = approvalEdits[approval.id] ?? JSON.stringify(approval.proposed_action, null, 2);
        let editedAction: unknown;
        try {
          editedAction = JSON.parse(editText);
        } catch (error) {
          throw new Error(`Edited action JSON is invalid: ${(error as Error).message}`);
        }
        await editLoopApproval(approval.id, editedAction);
      }
      await refreshApprovalSurfaces(approval.run_id);
      await refreshAutomations(true);
      setStatusMessage(`Loop approval ${action} finished for ${approval.action_type}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setAutomationOperation(null);
    }
  }

  async function handleMemoryProposalDecision(
    proposal: LoopMemoryProposalRecord,
    action: "approve" | "reject" | "edit",
  ) {
    setAutomationOperation(`${action} memory proposal`);
    try {
      if (action === "approve") {
        await approveLoopMemoryProposal(proposal.id);
      } else if (action === "reject") {
        await rejectLoopMemoryProposal(proposal.id);
      } else {
        const editText = proposalEdits[proposal.id] ?? JSON.stringify(proposal.candidate, null, 2);
        let editedCandidate: unknown;
        try {
          editedCandidate = JSON.parse(editText);
        } catch (error) {
          throw new Error(`Edited proposal JSON is invalid: ${(error as Error).message}`);
        }
        await editLoopMemoryProposal(proposal.id, editedCandidate);
      }
      await refreshApprovalSurfaces(proposal.run_id);
      await refreshAutomations(true);
      await refreshProject(project);
      setStatusMessage(`Memory proposal ${action} finished.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setAutomationOperation(null);
    }
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
    automationBusy: automationsLoading || automationOperation !== null || selectedLoopRunLoading,
    loopGlobalState,
    selectedLoopRun,
    selectedLoopRunApprovals,
    selectedLoopRunLoading,
    approvalQueue,
    approvalEdits,
    proposalEdits,
    refreshAutomations,
    handleSetLoopMode,
    handleDisableLoop,
    handlePauseLoop,
    handleSnoozeLoop,
    handleRunLoop,
    handleLoadLoopRun,
    setApprovalEdit,
    handleApprovalDecision,
    setProposalEdit,
    handleMemoryProposalDecision,
    handleToggleGlobalKillSwitch,
  };
}

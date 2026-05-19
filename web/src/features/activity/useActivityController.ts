import { useEffect, useState } from "react";

import { getActivities, getLlmAuditStatus, getUpToSpeed, setLlmAuditEnabled } from "../../api";
import type { ActivityEvent, LlmAuditStatusResponse, UpToSpeedResponse } from "../../types";
import { mergeActivityEventLists, mergeActivityEvents } from "../../utils/activity";

interface ActivityControllerOptions {
  project: string;
  activeTab: string;
  setStatusMessage: (message: string) => void;
  recordLocalDiagnostic: (component: string, operation: string, message: string) => void;
}

export function useActivityController({
  project,
  activeTab,
  setStatusMessage,
  recordLocalDiagnostic,
}: ActivityControllerOptions) {
  const [activities, setActivities] = useState<ActivityEvent[]>([]);
  const [selectedActivityIndex, setSelectedActivityIndex] = useState(0);
  const [upToSpeed, setUpToSpeed] = useState<UpToSpeedResponse | null>(null);
  const [upToSpeedLoading, setUpToSpeedLoading] = useState(false);
  const [upToSpeedError, setUpToSpeedError] = useState<string | null>(null);
  const [llmAudit, setLlmAudit] = useState<LlmAuditStatusResponse | null>(null);
  const [llmAuditLoading, setLlmAuditLoading] = useState(false);
  const [llmAuditError, setLlmAuditError] = useState<string | null>(null);
  const [llmAuditToggling, setLlmAuditToggling] = useState(false);

  useEffect(() => {
    setActivities([]);
    setSelectedActivityIndex(0);
    void getActivities(project, 100)
      .then((response) =>
        setActivities((current) => mergeActivityEventLists(response.items, current).slice(0, 200)),
      )
      .catch((error: Error) => {
        setStatusMessage(error.message);
        recordLocalDiagnostic("activity", "load", error.message);
      });
  }, [project, recordLocalDiagnostic, setStatusMessage]);

  useEffect(() => {
    if (activeTab !== "activity") return;
    void refreshLlmAuditStatus();
  }, [activeTab]);

  useEffect(() => {
    setSelectedActivityIndex((current) => Math.min(current, Math.max(activities.length - 1, 0)));
  }, [activities.length]);

  function addActivityEvent(event: ActivityEvent) {
    setActivities((current) => mergeActivityEvents(event, current).slice(0, 200));
  }

  async function handleUpToSpeed(includeLlmSummary: boolean) {
    setUpToSpeedLoading(true);
    setUpToSpeedError(null);
    try {
      const data = await getUpToSpeed({ project, include_llm_summary: includeLlmSummary, limit: 20 });
      setUpToSpeed(data);
      setStatusMessage(includeLlmSummary ? "LLM get-up-to-speed briefing loaded." : "Deterministic get-up-to-speed briefing loaded.");
    } catch (error) {
      const message = (error as Error).message;
      setUpToSpeedError(message);
      setStatusMessage(message);
      recordLocalDiagnostic("activity", "up_to_speed", message);
    } finally {
      setUpToSpeedLoading(false);
    }
  }

  async function refreshLlmAuditStatus() {
    setLlmAuditLoading(true);
    setLlmAuditError(null);
    try {
      const status = await getLlmAuditStatus();
      setLlmAudit(status);
    } catch (error) {
      const message = (error as Error).message;
      setLlmAuditError(message);
      recordLocalDiagnostic("activity", "llm_audit_status", message);
    } finally {
      setLlmAuditLoading(false);
    }
  }

  async function handleToggleLlmAudit() {
    const enabled = !(llmAudit?.enabled ?? false);
    setLlmAuditToggling(true);
    setLlmAuditError(null);
    try {
      const status = await setLlmAuditEnabled(enabled);
      setLlmAudit(status);
      setStatusMessage(`LLM audit/debug logging ${status.enabled ? "enabled" : "disabled"}.`);
    } catch (error) {
      const message = (error as Error).message;
      setLlmAuditError(message);
      setStatusMessage(message);
      recordLocalDiagnostic("activity", "llm_audit_toggle", message);
    } finally {
      setLlmAuditToggling(false);
    }
  }

  const activeActivity = activities[selectedActivityIndex] ?? null;

  return {
    activities,
    activeActivity,
    selectedActivityIndex,
    setSelectedActivityIndex,
    addActivityEvent,
    upToSpeed,
    upToSpeedLoading,
    upToSpeedError,
    llmAudit,
    llmAuditLoading,
    llmAuditError,
    llmAuditToggling,
    handleUpToSpeed,
    handleToggleLlmAudit,
  };
}

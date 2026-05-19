import { useEffect, useMemo, useState } from "react";

import { getAgentSnapshot } from "../../api";
import type { AgentSnapshotResponse } from "../../types";

interface AgentsControllerOptions {
  activeTab: string;
  project: string;
  effectiveRepoRoot: string;
}

export function useAgentsController({ activeTab, project, effectiveRepoRoot }: AgentsControllerOptions) {
  const [agentSnapshot, setAgentSnapshot] = useState<AgentSnapshotResponse | null>(null);
  const [selectedAgentIndex, setSelectedAgentIndex] = useState(0);

  useEffect(() => {
    if (activeTab !== "agents") return;
    let active = true;
    const poll = () => {
      void getAgentSnapshot()
        .then((snap) => {
          if (active) setAgentSnapshot(snap);
        })
        .catch(() => {});
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, [activeTab]);

  const sortedAgentSessions = useMemo(() => {
    const sessions = [...(agentSnapshot?.sessions ?? [])];
    sessions.sort((left, right) => {
      const leftCurrent = left.project_name === project || left.cwd === effectiveRepoRoot;
      const rightCurrent = right.project_name === project || right.cwd === effectiveRepoRoot;
      if (leftCurrent !== rightCurrent) return leftCurrent ? -1 : 1;
      return right.started_at - left.started_at;
    });
    return sessions;
  }, [agentSnapshot?.sessions, effectiveRepoRoot, project]);

  useEffect(() => {
    setSelectedAgentIndex((current) => Math.min(current, Math.max(sortedAgentSessions.length - 1, 0)));
  }, [sortedAgentSessions.length]);

  return {
    agentSnapshot,
    sortedAgentSessions,
    selectedAgent: sortedAgentSessions[selectedAgentIndex] ?? null,
    selectedAgentIndex,
    setSelectedAgentIndex,
  };
}

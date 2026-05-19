import { useEffect, useState } from "react";

import { getResume } from "../../api";
import type { ResumeResponse } from "../../types";

interface ResumeControllerOptions {
  project: string;
  effectiveRepoRoot: string;
  setTab: (tab: "resume") => void;
  setStatusMessage: (message: string) => void;
}

export function useResumeController({
  project,
  effectiveRepoRoot,
  setTab,
  setStatusMessage,
}: ResumeControllerOptions) {
  const [resumeData, setResumeData] = useState<ResumeResponse | null>(null);
  const [resumeLoading, setResumeLoading] = useState(false);
  const [resumeAutoloadedFor, setResumeAutoloadedFor] = useState<string | null>(null);

  useEffect(() => {
    if (!effectiveRepoRoot) return;
    const key = `${project}:${effectiveRepoRoot}`;
    if (resumeAutoloadedFor === key) return;
    setResumeAutoloadedFor(key);
    void getResume(project, effectiveRepoRoot, false)
      .then((data) => {
        setResumeData(data);
        if (data.checkpoint && (data.timeline.length || data.commits.length || data.changed_memories.length)) {
          setTab("resume");
        }
      })
      .catch(() => {});
  }, [effectiveRepoRoot, project, resumeAutoloadedFor, setTab]);

  async function handleLoadResume() {
    setResumeLoading(true);
    try {
      const data = await getResume(project, effectiveRepoRoot || null);
      setResumeData(data);
      setStatusMessage("Resume briefing loaded.");
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setResumeLoading(false);
    }
  }

  return {
    resumeData,
    resumeLoading,
    handleLoadResume,
  };
}

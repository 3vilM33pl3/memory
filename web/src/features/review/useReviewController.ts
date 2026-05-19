import { useEffect, useState } from "react";

import {
  approveProposal,
  getReplacementPolicy,
  getReplacementProposals,
  rejectProposal,
  saveReplacementPolicy,
} from "../../api";
import type { ReplacementPolicy, ReplacementPolicyResponse, ReplacementProposalRecord } from "../../types";

interface ReviewControllerOptions {
  activeTab: string;
  project: string;
  effectiveRepoRoot: string;
  repoRootInput: string;
  setRepoRootInput: (value: string) => void;
  setStatusMessage: (message: string) => void;
  refreshProject: (project: string) => Promise<void>;
}

export function useReviewController({
  activeTab,
  project,
  effectiveRepoRoot,
  repoRootInput,
  setRepoRootInput,
  setStatusMessage,
  refreshProject,
}: ReviewControllerOptions) {
  const [proposals, setProposals] = useState<ReplacementProposalRecord[]>([]);
  const [selectedProposalIndex, setSelectedProposalIndex] = useState(0);
  const [replacementPolicy, setReplacementPolicy] = useState<ReplacementPolicyResponse | null>(null);

  useEffect(() => {
    if (activeTab !== "review") return;
    void refreshReview();
  }, [activeTab, project]);

  async function refreshReview() {
    try {
      const [proposalPayload, policyPayload] = await Promise.all([
        getReplacementProposals(project),
        getReplacementPolicy(project, effectiveRepoRoot || null),
      ]);
      setProposals(proposalPayload.proposals);
      setSelectedProposalIndex((current) => Math.min(current, Math.max(proposalPayload.proposals.length - 1, 0)));
      setReplacementPolicy(policyPayload);
      if (!repoRootInput.trim() && policyPayload.repo_root) {
        setRepoRootInput(policyPayload.repo_root);
      }
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleCyclePolicy() {
    const repoRoot = replacementPolicy?.repo_root || effectiveRepoRoot;
    if (!repoRoot) {
      setStatusMessage("Set a repo root before changing the curation replacement policy.");
      return;
    }
    const current = replacementPolicy?.replacement_policy ?? "balanced";
    const next: ReplacementPolicy =
      current === "conservative" ? "balanced" : current === "balanced" ? "aggressive" : "conservative";
    try {
      const saved = await saveReplacementPolicy(project, {
        repo_root: repoRoot,
        replacement_policy: next,
      });
      setReplacementPolicy(saved);
      setRepoRootInput(saved.repo_root ?? repoRoot);
      setStatusMessage(`Curation replacement policy set to ${saved.replacement_policy}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleApproveProposal(proposalId: string) {
    try {
      const res = await approveProposal(project, proposalId);
      setStatusMessage(`Approved: ${res.candidate_summary} replaced ${res.target_summary}`);
      setProposals((prev) => prev.filter((p) => p.id !== proposalId));
      setSelectedProposalIndex((current) => Math.max(0, current - 1));
      await refreshProject(project);
      await refreshReview();
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleRejectProposal(proposalId: string) {
    try {
      const res = await rejectProposal(project, proposalId);
      setStatusMessage(`Rejected proposal for ${res.target_summary}`);
      setProposals((prev) => prev.filter((p) => p.id !== proposalId));
      setSelectedProposalIndex((current) => Math.max(0, current - 1));
      await refreshReview();
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  return {
    proposals,
    replacementPolicy,
    activeProposal: proposals[selectedProposalIndex] ?? null,
    selectedProposalIndex,
    setSelectedProposalIndex,
    refreshReview,
    handleCyclePolicy,
    handleApproveProposal,
    handleRejectProposal,
  };
}

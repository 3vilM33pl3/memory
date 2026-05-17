import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ReviewTab } from "./ReviewTab";
import type { ReplacementProposalRecord } from "../../types";

const proposal: ReplacementProposalRecord = {
  id: "proposal-1",
  project: "memory",
  target_memory_id: "target-1",
  target_summary: "Old routing memory",
  candidate_summary: "New route split memory",
  candidate_canonical_text: "# Route split\n- Service routes live in routes.rs",
  candidate_memory_type: "implementation",
  score: 0.92,
  policy: "balanced",
  reasons: ["same subsystem", "newer evidence"],
  created_at: "2026-05-17T09:00:00Z",
};

describe("ReviewTab", () => {
  it("renders proposal detail and dispatches review actions", () => {
    const onApproveProposal = vi.fn();
    const onRejectProposal = vi.fn();
    const onSelectProposal = vi.fn();

    render(
      <ReviewTab
        effectiveRepoRoot="/repo"
        proposals={[proposal]}
        activeProposal={proposal}
        selectedProposalIndex={0}
        replacementPolicy={{
          project: "memory",
          repo_root: "/repo",
          replacement_policy: "balanced",
          writable: true,
        }}
        onRefresh={vi.fn()}
        onCyclePolicy={vi.fn()}
        onSelectProposal={onSelectProposal}
        onApproveProposal={onApproveProposal}
        onRejectProposal={onRejectProposal}
      />,
    );

    expect(screen.getByRole("heading", { name: "Curation review" })).toBeInTheDocument();
    expect(screen.getByText(/Policy balanced/)).toBeInTheDocument();
    expect(screen.getAllByText("Old routing memory")).toHaveLength(2);
    expect(screen.getByText("Service routes live in routes.rs")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Select proposal Old routing memory" }));
    expect(onSelectProposal).toHaveBeenCalledWith(0);

    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    fireEvent.click(screen.getByRole("button", { name: "Reject" }));
    expect(onApproveProposal).toHaveBeenCalledWith("proposal-1");
    expect(onRejectProposal).toHaveBeenCalledWith("proposal-1");
  });
});

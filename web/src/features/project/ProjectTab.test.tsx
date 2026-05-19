import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { overviewFixture } from "../../test/fixtures";
import { ProjectTab } from "./ProjectTab";

describe("ProjectTab", () => {
  it("renders project overview metrics", () => {
    render(
      <ProjectTab
        project="memory"
        overview={overviewFixture}
        activities={[]}
        proposals={[]}
        replacementPolicy={null}
        onRefresh={vi.fn()}
        onProjectAction={vi.fn()}
        onOpenActivity={vi.fn()}
        onApproveProposal={vi.fn()}
        onRejectProposal={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Overview" })).toBeInTheDocument();
    expect(screen.getByText("not configured")).toBeInTheDocument();
  });
});

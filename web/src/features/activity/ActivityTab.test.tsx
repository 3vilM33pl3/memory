import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ActivityTab } from "./ActivityTab";

describe("ActivityTab", () => {
  it("renders briefing controls and empty activity detail", () => {
    render(
      <ActivityTab
        activities={[]}
        activeActivity={null}
        selectedActivityIndex={0}
        upToSpeed={null}
        upToSpeedLoading={false}
        upToSpeedError={null}
        llmAudit={null}
        llmAuditLoading={false}
        llmAuditError={null}
        llmAuditToggling={false}
        onLoadUpToSpeed={vi.fn()}
        onToggleLlmAudit={vi.fn()}
        onSelectActivity={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Get Up To Speed" })).toBeInTheDocument();
    expect(screen.getByText(/Keep this page open/)).toBeInTheDocument();
  });
});

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentsTab } from "./AgentsTab";

describe("AgentsTab", () => {
  it("renders the empty session state", () => {
    render(<AgentsTab agentSnapshot={null} sessions={[]} selectedAgent={null} selectedAgentIndex={0} onSelectAgent={vi.fn()} />);

    expect(screen.getByText("No agent sessions detected.")).toBeInTheDocument();
  });
});

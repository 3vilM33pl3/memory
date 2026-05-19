import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ResumeTab } from "./ResumeTab";

describe("ResumeTab", () => {
  it("renders the initial load prompt", () => {
    render(<ResumeTab resumeData={null} resumeLoading={false} onLoadResume={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Load resume" })).toBeInTheDocument();
    expect(screen.getByText(/generate a project briefing/)).toBeInTheDocument();
  });
});

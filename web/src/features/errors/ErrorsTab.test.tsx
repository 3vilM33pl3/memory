import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ErrorsTab } from "./ErrorsTab";

describe("ErrorsTab", () => {
  it("renders the empty diagnostics state", () => {
    render(<ErrorsTab errorItems={[]} activeError={null} selectedErrorIndex={0} onSelectError={vi.fn()} />);

    expect(screen.getByText("No diagnostics recorded for this project or browser session.")).toBeInTheDocument();
  });
});

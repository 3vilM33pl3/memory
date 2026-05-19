import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { overviewFixture } from "../../test/fixtures";
import { WatchersTab } from "./WatchersTab";

describe("WatchersTab", () => {
  it("renders the no watcher state", () => {
    render(<WatchersTab overview={overviewFixture} project="memory" />);

    expect(screen.getByRole("heading", { name: "Watcher presence" })).toBeInTheDocument();
    expect(screen.getByText("No watcher presence data.")).toBeInTheDocument();
  });
});

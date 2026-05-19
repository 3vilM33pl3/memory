import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { MemoriesTab } from "./MemoriesTab";

describe("MemoriesTab", () => {
  it("renders filter controls and empty detail", () => {
    render(
      <MemoriesTab
        searchRef={createRef<HTMLInputElement>()}
        filteredMemories={[]}
        selectedMemoryId={null}
        selectedMemory={null}
        selectedHistory={null}
        textFilter=""
        tagFilter=""
        statusFilter="all"
        typeFilter="all"
        onTextFilterChange={vi.fn()}
        onTagFilterChange={vi.fn()}
        onStatusFilterChange={vi.fn()}
        onTypeFilterChange={vi.fn()}
        onSelectMemory={vi.fn()}
        onClearHistory={vi.fn()}
        onLoadHistory={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByPlaceholderText("Search summary or preview (/)")).toBeInTheDocument();
    expect(screen.getByText("Select a memory to inspect its details.")).toBeInTheDocument();
  });
});

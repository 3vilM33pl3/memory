import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { QueryTab } from "./QueryTab";

describe("QueryTab", () => {
  it("renders the query input and empty results state", () => {
    render(
      <QueryTab
        queryRef={createRef<HTMLInputElement>()}
        queryText=""
        queryResponse={null}
        activeQueryResult={null}
        selectedQueryMemory={null}
        selectedQueryIndex={0}
        selectedQueryMemoryLoading={false}
        selectedQueryMemoryError={null}
        queryLoading={false}
        queryError={null}
        queryRoundtripMs={null}
        onQueryTextChange={vi.fn()}
        onSubmit={vi.fn()}
        onApplyHistory={vi.fn()}
        onResetHistoryCursor={vi.fn()}
        onSelectResult={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByPlaceholderText("Ask what the project knows... (?)")).toBeInTheDocument();
    expect(screen.getByText(/Run a query to inspect/)).toBeInTheDocument();
  });
});

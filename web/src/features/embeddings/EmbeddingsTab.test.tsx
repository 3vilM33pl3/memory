import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { EmbeddingsTab } from "./EmbeddingsTab";

describe("EmbeddingsTab", () => {
  it("renders the no backend state", () => {
    render(
      <EmbeddingsTab
        embeddingBackends={null}
        selectedEmbeddingBackend={null}
        selectedEmbeddingIndex={0}
        embeddingBusy={false}
        embeddingLoading={false}
        embeddingOperation={null}
        onRefresh={vi.fn()}
        onReindexAll={vi.fn()}
        onReembedAll={vi.fn()}
        onSelectBackend={vi.fn()}
        onToggleSearch={vi.fn()}
        onToggleCreation={vi.fn()}
        onReembedBackend={vi.fn()}
        onReindexBackend={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Embedding backends" })).toBeInTheDocument();
    expect(screen.getByText("No embedding backends configured.")).toBeInTheDocument();
  });
});

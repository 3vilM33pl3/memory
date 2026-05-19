import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { bundleOptionsFixture } from "../../test/fixtures";
import { BundlesTab } from "./BundlesTab";

describe("BundlesTab", () => {
  it("renders export and import panels", () => {
    render(
      <BundlesTab
        bundleOptions={bundleOptionsFixture}
        exportPreview={null}
        importPreview={null}
        onBundleOptionsChange={vi.fn()}
        onImportFileChange={vi.fn()}
        onPreviewExport={vi.fn()}
        onDownloadExport={vi.fn()}
        onPreviewImport={vi.fn()}
        onApplyImport={vi.fn()}
      />,
    );

    expect(screen.getByRole("heading", { name: "Export bundle" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Import bundle" })).toBeInTheDocument();
  });
});

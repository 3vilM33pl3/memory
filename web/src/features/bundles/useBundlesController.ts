import { useState } from "react";

import { exportBundle, importBundle, previewExportBundle, previewImportBundle } from "../../api";
import type { ProjectMemoryBundlePreview, ProjectMemoryExportOptions, ProjectMemoryImportPreview } from "../../types";

interface BundlesControllerOptions {
  project: string;
  setTab: (tab: "bundles") => void;
  setStatusMessage: (message: string) => void;
  refreshProject: (project: string) => Promise<void>;
}

export function useBundlesController({
  project,
  setTab,
  setStatusMessage,
  refreshProject,
}: BundlesControllerOptions) {
  const [bundleOptions, setBundleOptions] = useState<ProjectMemoryExportOptions>({
    include_archived: false,
    include_tags: true,
    include_relations: true,
    include_source_file_paths: false,
    include_git_commits: false,
    include_source_excerpts: false,
  });
  const [exportPreview, setExportPreview] = useState<ProjectMemoryBundlePreview | null>(null);
  const [importPreview, setImportPreview] = useState<ProjectMemoryImportPreview | null>(null);
  const [importFile, setImportFile] = useState<File | null>(null);

  async function handlePreviewExport() {
    try {
      const preview = await previewExportBundle(project, bundleOptions);
      setExportPreview(preview);
      setStatusMessage(`Prepared export preview for ${preview.memory_count} memories.`);
      setTab("bundles");
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleDownloadExport() {
    try {
      const blob = await exportBundle(project, bundleOptions);
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${project}-memory-bundle.zip`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
      setStatusMessage(`Downloaded export bundle for ${project}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handlePreviewImport() {
    if (!importFile) {
      setStatusMessage("Choose a bundle file first.");
      return;
    }
    try {
      const preview = await previewImportBundle(project, importFile);
      setImportPreview(preview);
      setStatusMessage(`Previewed bundle from ${preview.source_project}.`);
      setTab("bundles");
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  async function handleApplyImport() {
    if (!importFile) {
      setStatusMessage("Choose a bundle file first.");
      return;
    }
    try {
      const response = await importBundle(project, importFile);
      setImportPreview(null);
      setStatusMessage(`Imported ${response.imported_count} memories into ${response.target_project}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  return {
    bundleOptions,
    setBundleOptions,
    exportPreview,
    importPreview,
    setImportFile,
    handlePreviewExport,
    handleDownloadExport,
    handlePreviewImport,
    handleApplyImport,
  };
}

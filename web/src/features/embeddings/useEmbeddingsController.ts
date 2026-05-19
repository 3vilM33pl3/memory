import { useEffect, useState } from "react";

import {
  activateEmbeddingBackend,
  deactivateEmbeddingBackend,
  getEmbeddingBackends,
  reembed,
  reindex,
  setEmbeddingCreationEnabled,
} from "../../api";
import type { EmbeddingBackendInfo, EmbeddingBackendsResponse } from "../../types";

function embeddingBackendSelectionIndex(
  payload: EmbeddingBackendsResponse,
  preferredName: string | null,
  fallbackIndex: number,
): number {
  if (!payload.backends.length) return 0;
  if (preferredName) {
    const preferredIndex = payload.backends.findIndex((backend) => backend.name === preferredName);
    if (preferredIndex >= 0) return preferredIndex;
  }
  const activeIndex = payload.backends.findIndex((backend) => backend.active);
  if (activeIndex >= 0) return activeIndex;
  return Math.min(fallbackIndex, payload.backends.length - 1);
}

interface EmbeddingsControllerOptions {
  activeTab: string;
  project: string;
  setStatusMessage: (message: string) => void;
  refreshProject: (project: string) => Promise<void>;
}

export function useEmbeddingsController({
  activeTab,
  project,
  setStatusMessage,
  refreshProject,
}: EmbeddingsControllerOptions) {
  const [embeddingBackends, setEmbeddingBackends] = useState<EmbeddingBackendsResponse | null>(null);
  const [selectedEmbeddingIndex, setSelectedEmbeddingIndex] = useState(0);
  const [embeddingLoading, setEmbeddingLoading] = useState(false);
  const [embeddingOperation, setEmbeddingOperation] = useState<string | null>(null);

  useEffect(() => {
    if (activeTab !== "embeddings") return;
    void refreshEmbeddings();
  }, [activeTab, project]);

  async function refreshEmbeddings(preferredName?: string | null, quiet = false) {
    setEmbeddingLoading(true);
    try {
      const payload = await getEmbeddingBackends(project);
      const currentName = embeddingBackends?.backends[selectedEmbeddingIndex]?.name ?? null;
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) =>
        embeddingBackendSelectionIndex(payload, preferredName ?? currentName, current),
      );
      if (!quiet) setStatusMessage(`Loaded ${payload.backends.length} embedding backend(s).`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingLoading(false);
    }
  }

  async function handleToggleEmbeddingSearch(backend: EmbeddingBackendInfo) {
    if (!backend.ready) {
      setStatusMessage(`Embedding backend ${backend.name} is not ready.`);
      return;
    }
    setEmbeddingOperation(backend.active ? `turning off ${backend.name}` : `activating ${backend.name}`);
    try {
      const payload = backend.active
        ? await deactivateEmbeddingBackend()
        : await activateEmbeddingBackend(backend.name);
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) => embeddingBackendSelectionIndex(payload, backend.name, current));
      setStatusMessage(backend.active ? "Embeddings off." : `Activated embedding backend ${backend.name}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleToggleEmbeddingCreation(backend: EmbeddingBackendInfo) {
    const enabled = !backend.create_enabled;
    setEmbeddingOperation(`toggling automatic creation for ${backend.name}`);
    try {
      const payload = await setEmbeddingCreationEnabled(backend.name, enabled);
      setEmbeddingBackends(payload);
      setSelectedEmbeddingIndex((current) => embeddingBackendSelectionIndex(payload, backend.name, current));
      setStatusMessage(`Automatic embedding creation ${enabled ? "on" : "off"} for ${backend.name}.`);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleReembedEmbeddingBackend(backend: EmbeddingBackendInfo) {
    setEmbeddingOperation(`creating embeddings for ${backend.name}`);
    try {
      const response = await reembed(project, backend.name);
      setStatusMessage(`Created ${response.reembedded_chunks} chunk embedding(s) for ${backend.name}.`);
      await refreshEmbeddings(backend.name, true);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  async function handleReindexEmbeddingBackend(backend: EmbeddingBackendInfo) {
    setEmbeddingOperation(`reindexing ${backend.name}`);
    try {
      const response = await reindex(project, backend.name);
      setStatusMessage(`Reindexed ${response.reindexed_entries} memory entries for ${backend.name}.`);
      await refreshEmbeddings(backend.name, true);
      await refreshProject(project);
    } catch (error) {
      setStatusMessage((error as Error).message);
    } finally {
      setEmbeddingOperation(null);
    }
  }

  const selectedEmbeddingBackend = embeddingBackends?.backends[selectedEmbeddingIndex] ?? null;
  const embeddingBusy = embeddingLoading || embeddingOperation !== null;

  return {
    embeddingBackends,
    selectedEmbeddingBackend,
    selectedEmbeddingIndex,
    setSelectedEmbeddingIndex,
    embeddingBusy,
    embeddingLoading,
    embeddingOperation,
    refreshEmbeddings,
    handleToggleEmbeddingSearch,
    handleToggleEmbeddingCreation,
    handleReembedEmbeddingBackend,
    handleReindexEmbeddingBackend,
  };
}

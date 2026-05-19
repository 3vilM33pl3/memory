import { useEffect, useMemo, useState } from "react";

import { getMemory, getMemoryHistory } from "../../api";
import type { MemoryEntryResponse, MemoryHistoryResponse, ProjectMemoriesResponse } from "../../types";
import type { MemoryTypeFilter, StatusFilter } from "./types";

interface MemoriesControllerOptions {
  memories: ProjectMemoriesResponse;
  setStatusMessage: (message: string) => void;
  sendStream: (request: { type: "subscribe_memory"; memory_id: string } | { type: "unsubscribe_memory" }) => void;
}

export function useMemoriesController({
  memories,
  setStatusMessage,
  sendStream,
}: MemoriesControllerOptions) {
  const [selectedMemoryId, setSelectedMemoryId] = useState<string | null>(null);
  const [selectedMemory, setSelectedMemory] = useState<MemoryEntryResponse | null>(null);
  const [selectedHistory, setSelectedHistory] = useState<MemoryHistoryResponse | null>(null);
  const [textFilter, setTextFilter] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [typeFilter, setTypeFilter] = useState<MemoryTypeFilter>("all");

  const filteredMemories = useMemo(() => {
    return memories.items.filter((item) => {
      if (textFilter) {
        const haystack = `${item.summary} ${item.preview}`.toLowerCase();
        if (!haystack.includes(textFilter.toLowerCase())) return false;
      }
      if (tagFilter) {
        if (!item.tags.some((t) => t.toLowerCase().includes(tagFilter.toLowerCase()))) return false;
      }
      if (statusFilter !== "all" && item.status !== statusFilter) return false;
      if (typeFilter !== "all" && item.memory_type !== typeFilter) return false;
      return true;
    });
  }, [memories.items, statusFilter, tagFilter, textFilter, typeFilter]);

  useEffect(() => {
    if (!filteredMemories.length) {
      setSelectedMemoryId(null);
      return;
    }
    if (!selectedMemoryId || !filteredMemories.some((item) => item.id === selectedMemoryId)) {
      setSelectedMemoryId(filteredMemories[0].id);
    }
  }, [filteredMemories, selectedMemoryId]);

  useEffect(() => {
    if (!selectedMemoryId) {
      setSelectedMemory(null);
      setSelectedHistory(null);
      sendStream({ type: "unsubscribe_memory" });
      return;
    }
    setSelectedHistory(null);
    void getMemory(selectedMemoryId)
      .then(setSelectedMemory)
      .catch((error: Error) => setStatusMessage(error.message));
    sendStream({ type: "subscribe_memory", memory_id: selectedMemoryId });
  }, [selectedMemoryId, sendStream, setStatusMessage]);

  async function handleLoadHistory(memoryId: string) {
    try {
      if (selectedHistory) {
        setSelectedHistory(null);
        setStatusMessage("Hid version history.");
        return;
      }
      const history = await getMemoryHistory(memoryId);
      setSelectedHistory(history);
      setStatusMessage(`Loaded ${history.versions.length} versions for ${history.canonical_id}.`);
    } catch (error) {
      setStatusMessage((error as Error).message);
    }
  }

  return {
    filteredMemories,
    selectedMemoryId,
    selectedMemory,
    selectedHistory,
    textFilter,
    setTextFilter,
    tagFilter,
    setTagFilter,
    statusFilter,
    setStatusFilter,
    typeFilter,
    setTypeFilter,
    setSelectedMemoryId,
    setSelectedMemory,
    setSelectedHistory,
    handleLoadHistory,
  };
}

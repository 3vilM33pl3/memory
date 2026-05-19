import { useEffect, useRef } from "react";

import type { ActivityEvent, MemoryEntryResponse, ProjectMemoriesResponse, ProjectOverviewResponse, StreamRequest, StreamResponse } from "../types";
import { websocketUrl } from "../utils/network";

interface ProjectStreamOptions {
  project: string;
  selectedMemoryId: string | null;
  wsRef: React.MutableRefObject<WebSocket | null>;
  sendStream: (request: StreamRequest, socket?: WebSocket | null) => void;
  setConnectionState: (state: "connecting" | "live" | "offline") => void;
  setStatusMessage: (message: string) => void;
  setOverview: (overview: ProjectOverviewResponse) => void;
  setProjectMemories: (memories: ProjectMemoriesResponse) => void;
  setSelectedMemory: (memory: MemoryEntryResponse | null) => void;
  addActivityEvent: (event: ActivityEvent) => void;
  recordLocalDiagnostic: (component: string, operation: string, message: string) => void;
}

export function useProjectStream({
  project,
  selectedMemoryId,
  wsRef,
  sendStream,
  setConnectionState,
  setStatusMessage,
  setOverview,
  setProjectMemories,
  setSelectedMemory,
  addActivityEvent,
  recordLocalDiagnostic,
}: ProjectStreamOptions) {
  const callbacks = useRef({
    setOverview,
    setProjectMemories,
    setSelectedMemory,
    addActivityEvent,
    recordLocalDiagnostic,
    setStatusMessage,
  });

  useEffect(() => {
    callbacks.current = {
      setOverview,
      setProjectMemories,
      setSelectedMemory,
      addActivityEvent,
      recordLocalDiagnostic,
      setStatusMessage,
    };
  }, [addActivityEvent, recordLocalDiagnostic, setOverview, setProjectMemories, setSelectedMemory, setStatusMessage]);

  useEffect(() => {
    const socket = new WebSocket(websocketUrl());
    wsRef.current = socket;
    setConnectionState("connecting");

    socket.addEventListener("open", () => {
      setConnectionState("live");
      sendStream({ type: "subscribe_project", project }, socket);
      if (selectedMemoryId) {
        sendStream({ type: "subscribe_memory", memory_id: selectedMemoryId }, socket);
      }
    });

    socket.addEventListener("message", (event) => {
      const payload = JSON.parse(String(event.data)) as StreamResponse;
      if (payload.type === "project_snapshot" || payload.type === "project_changed") {
        callbacks.current.setOverview(payload.overview);
        callbacks.current.setProjectMemories(payload.memories);
      } else if (payload.type === "memory_snapshot" || payload.type === "memory_changed") {
        callbacks.current.setSelectedMemory(payload.detail);
      } else if (payload.type === "activity") {
        callbacks.current.addActivityEvent(payload.event);
      } else if (payload.type === "error") {
        callbacks.current.setStatusMessage(payload.message);
        callbacks.current.recordLocalDiagnostic("websocket", "stream", payload.message);
      }
    });

    socket.addEventListener("close", () => {
      setConnectionState("offline");
      callbacks.current.setStatusMessage("Live connection lost. The page still works, but updates are no longer streaming.");
      callbacks.current.recordLocalDiagnostic("websocket", "close", "Live connection lost.");
    });

    socket.addEventListener("error", () => {
      setConnectionState("offline");
      callbacks.current.recordLocalDiagnostic("websocket", "error", "WebSocket connection failed.");
    });

    return () => {
      socket.close();
      wsRef.current = null;
    };
  }, [project, selectedMemoryId, sendStream, setConnectionState, wsRef]);
}

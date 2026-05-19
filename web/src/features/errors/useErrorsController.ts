import { useEffect, useState } from "react";

import type { ActivityEvent, DiagnosticInfo } from "../../types";
import { collectErrorItems } from "./errorItems";

interface ErrorsControllerOptions {
  activities: ActivityEvent[];
  localDiagnostics: DiagnosticInfo[];
  connectionState: "connecting" | "live" | "offline";
}

export function useErrorsController({
  activities,
  localDiagnostics,
  connectionState,
}: ErrorsControllerOptions) {
  const [selectedErrorIndex, setSelectedErrorIndex] = useState(0);
  const errorItems = collectErrorItems(activities, localDiagnostics, connectionState);

  useEffect(() => {
    setSelectedErrorIndex((current) => Math.min(current, Math.max(errorItems.length - 1, 0)));
  }, [errorItems.length]);

  return {
    errorItems,
    activeError: errorItems[selectedErrorIndex] ?? null,
    selectedErrorIndex,
    setSelectedErrorIndex,
  };
}

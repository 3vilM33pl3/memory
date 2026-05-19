import type { ActivityEvent, DiagnosticInfo } from "../../types";

export interface ErrorItem {
  when: string | null;
  diagnostic: DiagnosticInfo;
}

export function collectErrorItems(
  activities: ActivityEvent[],
  localDiagnostics: DiagnosticInfo[],
  connectionState: "connecting" | "live" | "offline",
): ErrorItem[] {
  const items: ErrorItem[] = localDiagnostics.map((diagnostic) => ({ when: null, diagnostic }));
  if (connectionState === "offline") {
    items.push({
      when: null,
      diagnostic: {
        code: "backend_unavailable",
        source: "web",
        component: "service",
        operation: "stream",
        severity: "error",
        message: "Memory Layer backend live connection is unavailable.",
        raw_error: "WebSocket connection is offline.",
        explanation: "The browser can no longer receive live project updates from the backend stream.",
        fix_hint: "Check that the service is running, refresh the page, or run memory doctor.",
        doctor_hint: "memory doctor",
        command_hint: "memory service status",
      },
    });
  }
  for (const event of activities) {
    const details = event.details;
    if (details?.type === "diagnostic") {
      items.push({ when: event.recorded_at, diagnostic: details.diagnostic });
    } else if (details?.type === "query" && details.error) {
      items.push({
        when: event.recorded_at,
        diagnostic: {
          code: "query_error",
          source: event.source ?? "service",
          component: "query",
          operation: "query",
          severity: "error",
          message: details.error,
          raw_error: details.error,
          explanation: "A persisted project query failed.",
          fix_hint: "Open Query or Activity detail and run memory doctor if this repeats.",
          doctor_hint: "memory doctor",
          command_hint: "memory doctor",
        },
      });
    } else if (
      details?.type === "watcher_health" &&
      ["stale", "restarting", "failed"].includes(details.health)
    ) {
      items.push({
        when: event.recorded_at,
        diagnostic: {
          code: "watcher_health",
          source: event.source ?? "watcher",
          component: "watcher",
          operation: "heartbeat",
          severity: details.health === "failed" ? "error" : "warning",
          message: details.message ?? event.summary,
          raw_error: details.message ?? event.summary,
          explanation: "A watcher reported unhealthy or restarting state.",
          fix_hint: `Inspect watcher ${details.watcher_id} or run memory doctor.`,
          doctor_hint: "memory doctor",
          command_hint: "memory watcher status",
        },
      });
    } else if (event.kind === "query_error") {
      items.push({
        when: event.recorded_at,
        diagnostic: {
          code: "query_error",
          source: event.source ?? "service",
          component: "query",
          operation: "query",
          severity: "error",
          message: event.summary,
          raw_error: event.summary,
          explanation: "A persisted project query failed.",
          fix_hint: "Open the activity detail and run memory doctor if this repeats.",
          doctor_hint: "memory doctor",
          command_hint: "memory doctor",
        },
      });
    }
  }
  const fallbackTime = Date.now();
  return items.sort((left, right) => {
    const leftTime = left.when ? Date.parse(left.when) : fallbackTime;
    const rightTime = right.when ? Date.parse(right.when) : fallbackTime;
    return rightTime - leftTime;
  });
}

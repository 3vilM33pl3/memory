import type { ActivityEvent } from "../types";

export function mergeActivityEvents(event: ActivityEvent, current: ActivityEvent[]): ActivityEvent[] {
  return [event, ...current.filter((item) => item.id !== event.id)];
}

export function mergeActivityEventLists(primary: ActivityEvent[], secondary: ActivityEvent[]): ActivityEvent[] {
  const seen = new Set<string>();
  return [...primary, ...secondary].filter((event) => {
    if (seen.has(event.id)) return false;
    seen.add(event.id);
    return true;
  });
}

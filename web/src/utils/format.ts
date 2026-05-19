import type { ActivityEvent } from "../types";

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "n/a";
  return new Date(value).toLocaleString();
}

export function formatEpochSeconds(value: number | null | undefined): string {
  if (!value) return "n/a";
  return new Date(value * 1000).toLocaleString();
}

export function formatNumber(value: number | null | undefined): string {
  return typeof value === "number" ? value.toFixed(2) : "0.00";
}

export function formatPercent(value: number | null | undefined): string {
  return typeof value === "number" ? `${value.toFixed(0)}%` : "n/a";
}

export function formatCitationNumbers(values: number[]): string {
  return values.length ? values.map((value) => `[${value}]`).join(" ") : "none";
}

export function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}

export function activityTokenLabel(event: ActivityEvent): string {
  return event.token_usage ? `${formatTokens(event.token_usage.total_tokens)} tokens` : "tokens not recorded";
}

export function activityDurationLabel(event: ActivityEvent): string {
  return typeof event.duration_ms === "number" ? `${formatTokens(event.duration_ms)} ms` : "duration n/a";
}

export function formatElapsed(startedAtMs: number): string {
  const secs = Math.max(0, Math.floor((Date.now() - startedAtMs) / 1000));
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

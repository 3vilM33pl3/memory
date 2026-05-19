export const PRIMARY_TABS = [
  "memories",
  "agents",
  "query",
  "activity",
  "errors",
  "project",
  "review",
  "watchers",
  "embeddings",
  "resume",
] as const;

export const MORE_TABS = ["bundles"] as const;
export const ALL_TABS = [...PRIMARY_TABS, ...MORE_TABS] as const;

export type Tab = (typeof ALL_TABS)[number];

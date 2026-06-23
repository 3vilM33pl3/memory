export const PRIMARY_TABS = [
  "memories",
  "agents",
  "query",
  "graph",
  "activity",
  "errors",
  "project",
  "review",
  "watchers",
  "skills",
  "embeddings",
  "resume",
] as const;

export const MORE_TABS = ["automations", "bundles"] as const;
export const ALL_TABS = [...PRIMARY_TABS, ...MORE_TABS] as const;

export type Tab = (typeof ALL_TABS)[number];

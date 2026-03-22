import type {
  ArchiveResponse,
  CurateResponse,
  DeleteMemoryResponse,
  MemoryEntryResponse,
  ProjectMemoriesResponse,
  ProjectOverviewResponse,
  QueryRequest,
  QueryResponse,
  ReindexResponse,
} from "./types";

async function parseJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    throw new Error(`${response.status} ${await response.text()}`);
  }
  return (await response.json()) as T;
}

export async function getHealth(): Promise<Record<string, unknown>> {
  return parseJson(await fetch("/healthz"));
}

export async function getOverview(project: string): Promise<ProjectOverviewResponse> {
  return parseJson(await fetch(`/v1/projects/${encodeURIComponent(project)}/overview`));
}

export async function getMemories(project: string): Promise<ProjectMemoriesResponse> {
  return parseJson(await fetch(`/v1/projects/${encodeURIComponent(project)}/memories`));
}

export async function getMemory(memoryId: string): Promise<MemoryEntryResponse> {
  return parseJson(await fetch(`/v1/memory/${encodeURIComponent(memoryId)}`));
}

export async function runQuery(request: QueryRequest): Promise<QueryResponse> {
  return parseJson(
    await fetch("/v1/query", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function curate(project: string): Promise<CurateResponse> {
  return parseJson(
    await fetch("/v1/curate", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, batch_size: null }),
    }),
  );
}

export async function reindex(project: string): Promise<ReindexResponse> {
  return parseJson(
    await fetch("/v1/reindex", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project }),
    }),
  );
}

export async function archiveProject(project: string): Promise<ArchiveResponse> {
  return parseJson(
    await fetch("/v1/archive", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, max_confidence: 0.3, max_importance: 1 }),
    }),
  );
}

export async function deleteMemory(memoryId: string): Promise<DeleteMemoryResponse> {
  return parseJson(
    await fetch("/v1/memory", {
      method: "DELETE",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ memory_id: memoryId }),
    }),
  );
}

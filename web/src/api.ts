import type {
  AgentSnapshotResponse,
  ArchiveResponse,
  CurateResponse,
  DeleteMemoryResponse,
  MemoryEntryResponse,
  ProjectMemoriesResponse,
  ProjectMemoryBundlePreview,
  ProjectMemoryExportOptions,
  ProjectMemoryImportPreview,
  ProjectMemoryImportResponse,
  ProjectOverviewResponse,
  QueryRequest,
  QueryResponse,
  ReembedResponse,
  ReindexResponse,
  ReplacementProposalListResponse,
  ReplacementProposalResolutionResponse,
  ResumeResponse,
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

export async function reembed(project: string): Promise<ReembedResponse> {
  return parseJson(
    await fetch("/v1/reembed", {
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

export async function previewExportBundle(
  project: string,
  options: ProjectMemoryExportOptions,
): Promise<ProjectMemoryBundlePreview> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/bundle/export/preview`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(options),
    }),
  );
}

export async function exportBundle(
  project: string,
  options: ProjectMemoryExportOptions,
): Promise<Blob> {
  const response = await fetch(`/v1/projects/${encodeURIComponent(project)}/bundle/export`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(options),
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${await response.text()}`);
  }
  return response.blob();
}

export async function previewImportBundle(
  project: string,
  file: File,
): Promise<ProjectMemoryImportPreview> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/bundle/import/preview`, {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: file,
    }),
  );
}

export async function importBundle(
  project: string,
  file: File,
): Promise<ProjectMemoryImportResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/bundle/import`, {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: file,
    }),
  );
}

export async function getAgentSnapshot(): Promise<AgentSnapshotResponse> {
  return parseJson(await fetch("/v1/agents"));
}

export async function getResume(project: string): Promise<ResumeResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/resume`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, include_llm_summary: true, limit: 20 }),
    }),
  );
}

export async function getReplacementProposals(
  project: string,
): Promise<ReplacementProposalListResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/replacement-proposals`),
  );
}

export async function approveProposal(
  project: string,
  proposalId: string,
): Promise<ReplacementProposalResolutionResponse> {
  return parseJson(
    await fetch(
      `/v1/projects/${encodeURIComponent(project)}/replacement-proposals/${encodeURIComponent(proposalId)}/approve`,
      { method: "POST" },
    ),
  );
}

export async function rejectProposal(
  project: string,
  proposalId: string,
): Promise<ReplacementProposalResolutionResponse> {
  return parseJson(
    await fetch(
      `/v1/projects/${encodeURIComponent(project)}/replacement-proposals/${encodeURIComponent(proposalId)}/reject`,
      { method: "POST" },
    ),
  );
}

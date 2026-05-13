import type {
  AgentSnapshotResponse,
  ArchiveResponse,
  CurateResponse,
  DeleteMemoryResponse,
  EmbeddingBackendsResponse,
  MemoryEntryResponse,
  MemoryHistoryResponse,
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
  ReplacementPolicyRequest,
  ReplacementPolicyResponse,
  ReplacementProposalListResponse,
  ReplacementProposalResolutionResponse,
  ResumeResponse,
  ActivityListResponse,
  LlmAuditStatusResponse,
  RuntimeStatusResponse,
  UpToSpeedRequest,
  UpToSpeedResponse,
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

export async function getMemoryHistory(memoryId: string): Promise<MemoryHistoryResponse> {
  return parseJson(await fetch(`/v1/memory/${encodeURIComponent(memoryId)}/history`));
}

export async function getActivities(
  project: string,
  limit = 100,
  kind?: string | null,
): Promise<ActivityListResponse> {
  const params = new URLSearchParams({ limit: String(limit), include_details: "true" });
  if (kind) params.set("kind", kind);
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/activities?${params.toString()}`),
  );
}

export async function getRuntimeStatus(
  project: string,
  repoRoot?: string | null,
): Promise<RuntimeStatusResponse> {
  const params = new URLSearchParams({ project });
  if (repoRoot) params.set("repo_root", repoRoot);
  return parseJson(await fetch(`/v1/runtime/status?${params.toString()}`));
}

export async function getUpToSpeed(request: UpToSpeedRequest): Promise<UpToSpeedResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(request.project)}/up-to-speed`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function getLlmAuditStatus(): Promise<LlmAuditStatusResponse> {
  return parseJson(await fetch("/v1/config/llm-audit"));
}

export async function setLlmAuditEnabled(enabled: boolean): Promise<LlmAuditStatusResponse> {
  return parseJson(
    await fetch("/v1/config/llm-audit", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ enabled }),
    }),
  );
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

export async function reindex(project: string, backend?: string | null): Promise<ReindexResponse> {
  return parseJson(
    await fetch("/v1/reindex", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, backend: backend ?? null }),
    }),
  );
}

export async function reembed(project: string, backend?: string | null): Promise<ReembedResponse> {
  return parseJson(
    await fetch("/v1/reembed", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, backend: backend ?? null }),
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

export async function getResume(
  project: string,
  repoRoot?: string | null,
  includeLlmSummary = true,
): Promise<ResumeResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/resume`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, repo_root: repoRoot || null, include_llm_summary: includeLlmSummary, limit: 20 }),
    }),
  );
}

export async function getEmbeddingBackends(project: string): Promise<EmbeddingBackendsResponse> {
  const params = new URLSearchParams({ project });
  return parseJson(await fetch(`/v1/embeddings/backends?${params.toString()}`));
}

export async function activateEmbeddingBackend(name: string): Promise<EmbeddingBackendsResponse> {
  return parseJson(
    await fetch("/v1/embeddings/activate", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
    }),
  );
}

export async function deactivateEmbeddingBackend(): Promise<EmbeddingBackendsResponse> {
  return parseJson(
    await fetch("/v1/embeddings/deactivate", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    }),
  );
}

export async function setEmbeddingCreationEnabled(
  name: string,
  enabled: boolean,
): Promise<EmbeddingBackendsResponse> {
  return parseJson(
    await fetch("/v1/embeddings/create-enabled", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, enabled }),
    }),
  );
}

export async function getReplacementPolicy(
  project: string,
  repoRoot?: string | null,
): Promise<ReplacementPolicyResponse> {
  const params = new URLSearchParams();
  if (repoRoot) params.set("repo_root", repoRoot);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/replacement-policy${suffix}`),
  );
}

export async function saveReplacementPolicy(
  project: string,
  request: ReplacementPolicyRequest,
): Promise<ReplacementPolicyResponse> {
  return parseJson(
    await fetch(`/v1/projects/${encodeURIComponent(project)}/replacement-policy`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
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

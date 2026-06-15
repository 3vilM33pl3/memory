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
  LoopDefinitionResponse,
  LoopDefinitionsResponse,
  LoopApprovalDecisionResponse,
  LoopApprovalsResponse,
  LoopGlobalStateResponse,
  LoopGlobalStateUpdateRequest,
  LoopMemoryProposalDecisionResponse,
  LoopRunRequest,
  LoopRunResponse,
  LoopRunsResponse,
  LoopSettingsUpdateRequest,
  LoopSettingResponse,
  RuntimeStatusResponse,
  UpToSpeedRequest,
  UpToSpeedResponse,
} from "./types";

interface WebAuthTokenResponse {
  api_token: string;
  header: "x-api-token";
}

let webAuthTokenPromise: Promise<string> | null = null;

async function parseJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    throw new Error(`${response.status} ${await response.text()}`);
  }
  return (await response.json()) as T;
}

async function getWebAuthToken(): Promise<string> {
  if (!webAuthTokenPromise) {
    webAuthTokenPromise = parseJson<WebAuthTokenResponse>(await fetch("/v1/web/auth-token"))
      .then((payload) => payload.api_token)
      .catch((error) => {
        webAuthTokenPromise = null;
        throw error;
      });
  }
  return webAuthTokenPromise;
}

function withAuthHeader(headers: HeadersInit | undefined, token: string): Headers {
  const merged = new Headers(headers);
  merged.set("x-api-token", token);
  return merged;
}

async function apiFetch(input: RequestInfo | URL, init: RequestInit = {}): Promise<Response> {
  const token = await getWebAuthToken();
  return fetch(input, { ...init, headers: withAuthHeader(init.headers, token) });
}

export async function getHealth(): Promise<Record<string, unknown>> {
  return parseJson(await fetch("/healthz"));
}

export async function getOverview(project: string): Promise<ProjectOverviewResponse> {
  return parseJson(await apiFetch(`/v1/projects/${encodeURIComponent(project)}/overview`));
}

export async function getMemories(project: string): Promise<ProjectMemoriesResponse> {
  return parseJson(await apiFetch(`/v1/projects/${encodeURIComponent(project)}/memories`));
}

export async function getMemory(memoryId: string): Promise<MemoryEntryResponse> {
  return parseJson(await apiFetch(`/v1/memory/${encodeURIComponent(memoryId)}`));
}

export async function getMemoryHistory(memoryId: string): Promise<MemoryHistoryResponse> {
  return parseJson(await apiFetch(`/v1/memory/${encodeURIComponent(memoryId)}/history`));
}

export async function getActivities(
  project: string,
  limit = 100,
  kind?: string | null,
): Promise<ActivityListResponse> {
  const params = new URLSearchParams({ limit: String(limit), include_details: "true" });
  if (kind) params.set("kind", kind);
  return parseJson(
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/activities?${params.toString()}`),
  );
}

export async function getRuntimeStatus(
  project: string,
  repoRoot?: string | null,
): Promise<RuntimeStatusResponse> {
  const params = new URLSearchParams({ project });
  if (repoRoot) params.set("repo_root", repoRoot);
  return parseJson(await apiFetch(`/v1/runtime/status?${params.toString()}`));
}

export async function getUpToSpeed(request: UpToSpeedRequest): Promise<UpToSpeedResponse> {
  return parseJson(
    await apiFetch(`/v1/projects/${encodeURIComponent(request.project)}/up-to-speed`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function getLlmAuditStatus(): Promise<LlmAuditStatusResponse> {
  return parseJson(await apiFetch("/v1/config/llm-audit"));
}

export async function setLlmAuditEnabled(enabled: boolean): Promise<LlmAuditStatusResponse> {
  return parseJson(
    await apiFetch("/v1/config/llm-audit", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ enabled }),
    }),
  );
}

export async function runQuery(request: QueryRequest): Promise<QueryResponse> {
  return parseJson(
    await apiFetch("/v1/query", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function curate(project: string): Promise<CurateResponse> {
  return parseJson(
    await apiFetch("/v1/curate", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, batch_size: null }),
    }),
  );
}

export async function reindex(project: string, backend?: string | null): Promise<ReindexResponse> {
  return parseJson(
    await apiFetch("/v1/reindex", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, backend: backend ?? null }),
    }),
  );
}

export async function reembed(project: string, backend?: string | null): Promise<ReembedResponse> {
  return parseJson(
    await apiFetch("/v1/reembed", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, backend: backend ?? null }),
    }),
  );
}

export async function archiveProject(project: string): Promise<ArchiveResponse> {
  return parseJson(
    await apiFetch("/v1/archive", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, max_confidence: 0.3, max_importance: 1 }),
    }),
  );
}

export async function deleteMemory(memoryId: string): Promise<DeleteMemoryResponse> {
  return parseJson(
    await apiFetch("/v1/memory", {
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
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/bundle/export/preview`, {
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
  const response = await apiFetch(`/v1/projects/${encodeURIComponent(project)}/bundle/export`, {
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
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/bundle/import/preview`, {
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
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/bundle/import`, {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: file,
    }),
  );
}

export async function getAgentSnapshot(): Promise<AgentSnapshotResponse> {
  return parseJson(await apiFetch("/v1/agents"));
}

export async function getResume(
  project: string,
  repoRoot?: string | null,
  includeLlmSummary = true,
): Promise<ResumeResponse> {
  return parseJson(
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/resume`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ project, repo_root: repoRoot || null, include_llm_summary: includeLlmSummary, limit: 20 }),
    }),
  );
}

export async function getEmbeddingBackends(project: string): Promise<EmbeddingBackendsResponse> {
  const params = new URLSearchParams({ project });
  return parseJson(await apiFetch(`/v1/embeddings/backends?${params.toString()}`));
}

export async function activateEmbeddingBackend(name: string): Promise<EmbeddingBackendsResponse> {
  return parseJson(
    await apiFetch("/v1/embeddings/activate", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name }),
    }),
  );
}

export async function deactivateEmbeddingBackend(): Promise<EmbeddingBackendsResponse> {
  return parseJson(
    await apiFetch("/v1/embeddings/deactivate", {
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
    await apiFetch("/v1/embeddings/create-enabled", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, enabled }),
    }),
  );
}

export async function listLoopDefinitions(): Promise<LoopDefinitionsResponse> {
  return parseJson(await apiFetch("/v1/loops"));
}

export async function getLoopDefinition(
  loopId: string,
  project?: string | null,
  repoRoot?: string | null,
): Promise<LoopDefinitionResponse> {
  const params = new URLSearchParams();
  if (project) params.set("project", project);
  if (repoRoot) params.set("repo_root", repoRoot);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return parseJson(await apiFetch(`/v1/loops/${encodeURIComponent(loopId)}${suffix}`));
}

export async function getLoopRuns(options: {
  project?: string | null;
  loopId?: string | null;
  limit?: number;
}): Promise<LoopRunsResponse> {
  const params = new URLSearchParams();
  if (options.project) params.set("project", options.project);
  if (options.loopId) params.set("loop_id", options.loopId);
  if (options.limit) params.set("limit", String(options.limit));
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return parseJson(await apiFetch(`/v1/loops/runs${suffix}`));
}

export async function getLoopRun(runId: string): Promise<LoopRunResponse> {
  return parseJson(await apiFetch(`/v1/loops/runs/${encodeURIComponent(runId)}`));
}

export async function getLoopApprovals(options: {
  project?: string | null;
  runId?: string | null;
  loopId?: string | null;
  status?: string | null;
  limit?: number;
}): Promise<LoopApprovalsResponse> {
  const params = new URLSearchParams();
  if (options.project) params.set("project", options.project);
  if (options.runId) params.set("run_id", options.runId);
  if (options.loopId) params.set("loop_id", options.loopId);
  if (options.status) params.set("status", options.status);
  if (options.limit) params.set("limit", String(options.limit));
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return parseJson(await apiFetch(`/v1/loops/approvals${suffix}`));
}

async function postLoopApprovalDecision(
  approvalId: string,
  action: "approve" | "reject" | "edit",
  request: { reviewer?: string | null; reason?: string | null; edited_action?: unknown },
): Promise<LoopApprovalDecisionResponse> {
  return parseJson(
    await apiFetch(`/v1/loops/approvals/${encodeURIComponent(approvalId)}/${action}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function approveLoopApproval(
  approvalId: string,
  reason?: string,
): Promise<LoopApprovalDecisionResponse> {
  return postLoopApprovalDecision(approvalId, "approve", {
    reviewer: "web",
    reason: reason || "Approved from the browser UI.",
  });
}

export async function rejectLoopApproval(
  approvalId: string,
  reason?: string,
): Promise<LoopApprovalDecisionResponse> {
  return postLoopApprovalDecision(approvalId, "reject", {
    reviewer: "web",
    reason: reason || "Rejected from the browser UI.",
  });
}

export async function editLoopApproval(
  approvalId: string,
  editedAction: unknown,
  reason?: string,
): Promise<LoopApprovalDecisionResponse> {
  return postLoopApprovalDecision(approvalId, "edit", {
    reviewer: "web",
    reason: reason || "Edited from the browser UI.",
    edited_action: editedAction,
  });
}

async function postLoopMemoryProposalDecision(
  proposalId: string,
  action: "approve" | "reject" | "edit",
  request: {
    reviewer?: string | null;
    reason?: string | null;
    edited_candidate?: unknown;
    edited_evidence?: unknown;
    edited_risk_notes?: string | null;
  },
): Promise<LoopMemoryProposalDecisionResponse> {
  return parseJson(
    await apiFetch(`/v1/loops/memory-proposals/${encodeURIComponent(proposalId)}/${action}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function approveLoopMemoryProposal(
  proposalId: string,
  reason?: string,
): Promise<LoopMemoryProposalDecisionResponse> {
  return postLoopMemoryProposalDecision(proposalId, "approve", {
    reviewer: "web",
    reason: reason || "Approved from the browser UI.",
  });
}

export async function rejectLoopMemoryProposal(
  proposalId: string,
  reason?: string,
): Promise<LoopMemoryProposalDecisionResponse> {
  return postLoopMemoryProposalDecision(proposalId, "reject", {
    reviewer: "web",
    reason: reason || "Rejected from the browser UI.",
  });
}

export async function editLoopMemoryProposal(
  proposalId: string,
  editedCandidate: unknown,
  reason?: string,
): Promise<LoopMemoryProposalDecisionResponse> {
  return postLoopMemoryProposalDecision(proposalId, "edit", {
    reviewer: "web",
    reason: reason || "Edited from the browser UI.",
    edited_candidate: editedCandidate,
  });
}

export async function getLoopGlobalState(): Promise<LoopGlobalStateResponse> {
  return parseJson(await apiFetch("/v1/loops/global-kill-switch"));
}

export async function updateLoopGlobalState(
  request: LoopGlobalStateUpdateRequest,
): Promise<LoopGlobalStateResponse> {
  return parseJson(
    await apiFetch("/v1/loops/global-kill-switch", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

async function postLoopSetting(
  loopId: string,
  action: "enable" | "disable" | "pause" | "snooze",
  request: LoopSettingsUpdateRequest,
): Promise<LoopSettingResponse> {
  return parseJson(
    await apiFetch(`/v1/loops/${encodeURIComponent(loopId)}/${action}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    }),
  );
}

export async function enableLoop(
  loopId: string,
  request: LoopSettingsUpdateRequest,
): Promise<LoopSettingResponse> {
  return postLoopSetting(loopId, "enable", request);
}

export async function disableLoop(
  loopId: string,
  request: LoopSettingsUpdateRequest,
): Promise<LoopSettingResponse> {
  return postLoopSetting(loopId, "disable", request);
}

export async function pauseLoop(
  loopId: string,
  request: LoopSettingsUpdateRequest,
): Promise<LoopSettingResponse> {
  return postLoopSetting(loopId, "pause", request);
}

export async function snoozeLoop(
  loopId: string,
  request: LoopSettingsUpdateRequest,
): Promise<LoopSettingResponse> {
  return postLoopSetting(loopId, "snooze", request);
}

export async function runLoop(loopId: string, request: LoopRunRequest): Promise<LoopRunResponse> {
  return parseJson(
    await apiFetch(`/v1/loops/${encodeURIComponent(loopId)}/run`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
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
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/replacement-policy${suffix}`),
  );
}

export async function saveReplacementPolicy(
  project: string,
  request: ReplacementPolicyRequest,
): Promise<ReplacementPolicyResponse> {
  return parseJson(
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/replacement-policy`, {
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
    await apiFetch(`/v1/projects/${encodeURIComponent(project)}/replacement-proposals`),
  );
}

export async function approveProposal(
  project: string,
  proposalId: string,
): Promise<ReplacementProposalResolutionResponse> {
  return parseJson(
    await apiFetch(
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
    await apiFetch(
      `/v1/projects/${encodeURIComponent(project)}/replacement-proposals/${encodeURIComponent(proposalId)}/reject`,
      { method: "POST" },
    ),
  );
}

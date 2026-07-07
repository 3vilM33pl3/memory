/**
 * Typed TypeScript client for the Memory Layer HTTP API v1 — the frozen
 * `x-stability: core` surface (see GET /v1/openapi.yaml on a running
 * service). Core v1 is additive-only: unknown response fields are preserved
 * on the returned objects and must be ignored, never rejected.
 *
 * Zero runtime dependencies (global fetch, Node >= 18 or any browser).
 */

export const DEFAULT_BASE_URL = "http://127.0.0.1:4040";

export class MemoryLayerError extends Error {
  readonly status: number;
  readonly body: string;

  constructor(status: number, body: string) {
    super(`HTTP ${status}: ${body}`);
    this.status = status;
    this.body = body;
  }
}

export interface QueryResult {
  memory_id: string;
  summary: string;
  memory_type: string;
  score: number;
  snippet: string;
  tags?: string[];
  [extra: string]: unknown;
}

export interface QueryAnswer {
  answer: string;
  confidence: number;
  insufficient_evidence: boolean;
  results: QueryResult[];
  answer_citations?: Array<{ result_number: number; memory_id: string; [extra: string]: unknown }>;
  [extra: string]: unknown;
}

export interface MemoryGraphNode {
  id: string;
  label: string;
  node_kind: "memory" | "source";
  memory_id?: string | null;
  memory_type?: string | null;
  confidence?: number | null;
  importance?: number | null;
  tags?: string[];
  /** Decayed ACT-R activation at read time; null/absent = never retrieved. */
  activation?: number | null;
  [extra: string]: unknown;
}

export interface MemoryGraph {
  project: string;
  total_memories: number;
  returned_memories: number;
  nodes: MemoryGraphNode[];
  edges: Array<{ id: string; source: string; target: string; edge_kind: string; [extra: string]: unknown }>;
  [extra: string]: unknown;
}

export interface QueryOptions {
  topK?: number;
  tags?: string[];
  /** Force the deterministic (keyless, reproducible) synthesizer. */
  deterministic?: boolean;
}

export interface RememberOptions {
  title: string;
  summary: string;
  notes?: string[];
  memoryType?: string;
  tags?: string[];
  confidence?: number;
  importance?: number;
}

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export class MemoryLayerClient {
  private readonly baseUrl: string;
  private readonly token: string;
  private readonly fetchImpl: FetchLike;
  private readonly writerId: string;

  constructor(options?: {
    baseUrl?: string;
    token?: string;
    fetchImpl?: FetchLike;
    writerId?: string;
  }) {
    this.baseUrl = (options?.baseUrl ?? DEFAULT_BASE_URL).replace(/\/$/, "");
    this.token = options?.token ?? "";
    this.fetchImpl = options?.fetchImpl ?? fetch;
    this.writerId = options?.writerId ?? "typescript-client";
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method,
      headers: {
        "content-type": "application/json",
        "x-api-token": this.token,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) {
      throw new MemoryLayerError(response.status, await response.text());
    }
    return (await response.json()) as T;
  }

  // -- core reads ----------------------------------------------------

  health(): Promise<Record<string, unknown>> {
    return this.request("GET", "/healthz");
  }

  stats(): Promise<Record<string, unknown>> {
    return this.request("GET", "/v1/stats");
  }

  query(project: string, question: string, options?: QueryOptions): Promise<QueryAnswer> {
    const payload: Record<string, unknown> = {
      project,
      query: question,
      top_k: options?.topK ?? 8,
      include_stale: false,
      history: false,
    };
    if (options?.tags?.length) payload.filters = { tags: options.tags };
    if (options?.deterministic) payload.answer_mode = "deterministic";
    return this.request("POST", "/v1/query", payload);
  }

  queryGlobal(question: string, topK = 8): Promise<QueryAnswer> {
    return this.request("POST", "/v1/query/global", { query: question, top_k: topK });
  }

  memory(memoryId: string): Promise<Record<string, unknown>> {
    return this.request("GET", `/v1/memory/${memoryId}`);
  }

  memoryHistory(memoryId: string): Promise<Record<string, unknown>> {
    return this.request("GET", `/v1/memory/${memoryId}/history`);
  }

  projectMemories(project: string): Promise<Record<string, unknown>> {
    return this.request("GET", `/v1/projects/${project}/memories`);
  }

  memoryGraph(project: string, limit = 250): Promise<MemoryGraph> {
    return this.request("GET", `/v1/projects/${project}/memory-graph?limit=${limit}`);
  }

  overview(project: string): Promise<Record<string, unknown>> {
    return this.request("GET", `/v1/projects/${project}/overview`);
  }

  resume(project: string): Promise<Record<string, unknown>> {
    return this.request("POST", `/v1/projects/${project}/resume`, { project });
  }

  // -- core writes ---------------------------------------------------

  /** Low-level capture; see the OpenAPI CaptureTaskRequest schema. */
  captureTask(request: Record<string, unknown>): Promise<{ raw_capture_id: string; [extra: string]: unknown }> {
    return this.request("POST", "/v1/capture/task", request);
  }

  /** Curate; pass rawCaptureId for bounded single-capture curation. */
  curate(project: string, rawCaptureId?: string): Promise<Record<string, unknown>> {
    const payload: Record<string, unknown> = { project };
    if (rawCaptureId) payload.raw_capture_id = rawCaptureId;
    return this.request("POST", "/v1/curate", payload);
  }

  /** Capture one durable fact and curate it, in one call. */
  async remember(project: string, options: RememberOptions): Promise<Record<string, unknown>> {
    const canonical = options.notes?.length
      ? `${options.summary} ${options.notes.join(" ")}`
      : options.summary;
    const capture = await this.captureTask({
      project,
      task_title: options.title,
      user_prompt: options.title,
      writer_id: this.writerId,
      agent_summary: options.summary,
      structured_candidates: [
        {
          canonical_text: canonical,
          summary: options.summary,
          memory_type: options.memoryType ?? "project",
          confidence: options.confidence ?? 0.85,
          importance: options.importance ?? 3,
          tags: options.tags ?? [],
          sources: [{ source_kind: "note", excerpt: options.title }],
        },
      ],
    });
    return this.curate(project, capture.raw_capture_id);
  }
}

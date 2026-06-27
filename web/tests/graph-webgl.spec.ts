import { expect, test, type Locator, type Page } from "@playwright/test";
import { PNG } from "pngjs";

test.beforeEach(async ({ page }) => {
  await mockMemoryApi(page);
});

test("Graph tab renders a nonblank WebGL scene without layout overlap", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "graph" }).click();

  const scene = page.getByTestId("graph-scene");
  await expect(scene).toBeVisible();
  const canvas = scene.locator("canvas").first();
  await expect(canvas).toBeVisible();
  await expect(page.getByText("showing 5 / 3")).toBeVisible();
  await expect(page.getByRole("button", { name: "Back" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Forward" })).toBeDisabled();

  await page.waitForTimeout(1500);
  await expect(await canvasHasMultipleColors(canvas)).toBeTruthy();

  await page.getByRole("checkbox", { name: "Isolate connected graph" }).check();
  await expect(page.getByText("showing 2 / 1 within 1 degree from 5 / 3")).toBeVisible();
  await page.getByRole("button", { name: "Increase graph degrees" }).click();
  await expect(page.getByText("showing 3 / 2 within 2 degrees from 5 / 3")).toBeVisible();
  await page.getByRole("button", { name: "Increase graph degrees" }).click();
  await expect(page.getByText("showing 4 / 3 within 3 degrees from 5 / 3")).toBeVisible();
  await page.waitForTimeout(500);
  await expect(await canvasHasMultipleColors(canvas)).toBeTruthy();

  const canvasBox = await canvas.boundingBox();
  expect(canvasBox).not.toBeNull();
  if (canvasBox) {
    await canvas.click({
      position: {
        x: Math.floor(canvasBox.width / 2),
        y: Math.floor(canvasBox.height / 2),
      },
    });
  }
  await page.waitForTimeout(500);
  const clickedCanvas = scene.locator("canvas").first();
  await expect(clickedCanvas).toBeVisible();
  await expect(await canvasHasMultipleColors(clickedCanvas)).toBeTruthy();

  const toolbarBox = await page.locator(".graph-toolbar").boundingBox();
  const sceneBox = await scene.boundingBox();
  const inspectorBox = await page.locator(".graph-inspector").boundingBox();
  expect(toolbarBox).not.toBeNull();
  expect(sceneBox).not.toBeNull();
  expect(inspectorBox).not.toBeNull();
  if (toolbarBox && sceneBox) {
    expect(toolbarBox.y + toolbarBox.height).toBeLessThanOrEqual(sceneBox.y + 96);
  }
  if (sceneBox && inspectorBox) {
    const sideBySide = sceneBox.x + sceneBox.width <= inspectorBox.x + 1;
    const stacked = sceneBox.y + sceneBox.height <= inspectorBox.y + 1;
    expect(sideBySide || stacked).toBeTruthy();
  }
});

async function mockMemoryApi(page: Page) {
  await page.route("**/v1/web/auth-token", (route) =>
    route.fulfill({ json: { api_token: "test-token", header: "x-api-token" } }),
  );
  await page.route("**/healthz", (route) =>
    route.fulfill({ json: { status: "ok", version: "test" } }),
  );
  await page.route("**/v1/projects/memory/overview", (route) =>
    route.fulfill({ json: overviewResponse }),
  );
  await page.route("**/v1/projects/memory/memories", (route) =>
    route.fulfill({ json: { project: "memory", total: 0, items: [] } }),
  );
  await page.route("**/v1/projects/memory/activities?**", (route) =>
    route.fulfill({ json: { project: "memory", total_returned: 0, items: [] } }),
  );
  await page.route("**/v1/runtime/status?**", (route) =>
    route.fulfill({ json: runtimeStatusResponse }),
  );
  await page.route("**/v1/projects/memory/graph/status", (route) =>
    route.fulfill({ json: graphStatusResponse }),
  );
  await page.route("**/v1/projects/memory/graph?**", (route) =>
    route.fulfill({ json: graphResponse }),
  );
}

async function canvasHasMultipleColors(canvas: Locator): Promise<boolean> {
  const png = PNG.sync.read(await canvas.screenshot());
  if (png.width === 0 || png.height === 0) return false;
  const base = [png.data[0], png.data[1], png.data[2]];
  let differentPixels = 0;
  for (let index = 0; index < png.data.length; index += 4) {
    const alpha = png.data[index + 3];
    if (alpha === 0) continue;
    const delta =
      Math.abs(png.data[index] - base[0]) +
      Math.abs(png.data[index + 1] - base[1]) +
      Math.abs(png.data[index + 2] - base[2]);
    if (delta > 24) differentPixels += 1;
    if (differentPixels > 50) return true;
  }
  return false;
}

const overviewResponse = {
  project: "memory",
  service_status: "ok",
  database_status: "ok",
  memory_entries_total: 0,
  active_memories: 0,
  archived_memories: 0,
  high_confidence_memories: 0,
  medium_confidence_memories: 0,
  low_confidence_memories: 0,
  recent_memories_7d: 0,
  recent_captures_7d: 0,
  raw_captures_total: 0,
  uncurated_raw_captures: 0,
  tasks_total: 0,
  sessions_total: 0,
  curation_runs_total: 0,
  last_memory_at: null,
  last_curation_at: null,
  last_capture_at: null,
  oldest_uncurated_capture_age_hours: null,
  embedding_chunks_total: 0,
  fresh_embedding_chunks: 0,
  stale_embedding_chunks: 0,
  missing_embedding_chunks: 0,
  embedding_spaces_total: 0,
  active_embedding_provider: null,
  active_embedding_model: null,
  pending_replacement_proposals: 0,
  top_tags: [],
  top_files: [],
  memory_type_breakdown: [],
  source_kind_breakdown: [],
  automation: null,
  watchers: null,
};

const runtimeStatusResponse = {
  generated_at: "2026-06-23T16:00:00Z",
  project: "memory",
  profile: "test",
  web: { version: "test", status: "ok" },
  service: { version: "test", status: "ok" },
  manager: {
    version: "test",
    state: "active",
    tracked_sessions: 0,
    warning_count: 0,
    event_count: 0,
    fallback_scan_count: 0,
  },
  watchers: {
    version: "test",
    status: "ok",
    active_count: 0,
    unhealthy_count: 0,
    stale_after_seconds: 90,
  },
  provenance: {
    status: "ok",
    enabled: true,
    interval_seconds: 86400,
    checked_count: 0,
    stale_count: 0,
  },
  skills: {
    bundle_version: "test",
    status: "ok",
    summary: "ok",
    filter: "memory-layer",
    details: [],
  },
  restart_notice: null,
};

const graphStatusResponse = {
  project: "memory",
  has_graph: true,
  latest_run_id: "11111111-1111-4111-8111-111111111111",
  repo_root: "/repo",
  git_head: "abc",
  analyzer_version: "mem-analyze-v2",
  strategy_version: "code-graph-resolution-v1",
  status: "completed",
  completed_at: "2026-06-23T16:00:00Z",
  symbol_count: 5,
  reference_count: 3,
  resolved_reference_count: 3,
  unresolved_reference_count: 0,
  ambiguous_reference_count: 0,
  graph_node_count: 5,
  graph_edge_count: 3,
  evidence_count: 5,
};

const graphResponse = {
  project: "memory",
  status: graphStatusResponse,
  filters: { run_id: graphStatusResponse.latest_run_id, depth: 1, limit_nodes: 250, limit_edges: 500 },
  stats: {
    total_nodes: 5,
    total_edges: 3,
    total_symbols: 5,
    total_references: 3,
    unresolved_references: 0,
    returned_nodes: 5,
    returned_edges: 3,
    seed_nodes: 1,
  },
  truncated: false,
  nodes: [
    {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      stable_identity: "rust:src/handler.rs:function:GraphHandler:10-20",
      label: "GraphHandler",
      node_kind: "code_symbol",
      language: "rust",
      symbol_kind: "function",
      file_path: "src/handler.rs",
      name: "GraphHandler",
      qualified_name: "GraphHandler",
      start_line: 10,
      end_line: 20,
      degree: 2,
      seed: true,
      group: "rust",
    },
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      stable_identity: "rust:src/repository.rs:function:GraphRepository:30-40",
      label: "GraphRepository",
      node_kind: "code_symbol",
      language: "rust",
      symbol_kind: "function",
      file_path: "src/repository.rs",
      name: "GraphRepository",
      qualified_name: "GraphRepository",
      start_line: 30,
      end_line: 40,
      degree: 1,
      seed: false,
      group: "rust",
    },
    {
      id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      stable_identity: "ts:web/src/GraphTab.tsx:function:GraphTab:5-25",
      label: "GraphTab",
      node_kind: "code_symbol",
      language: "typescript",
      symbol_kind: "component",
      file_path: "web/src/GraphTab.tsx",
      name: "GraphTab",
      qualified_name: "GraphTab",
      start_line: 5,
      end_line: 25,
      degree: 1,
      seed: false,
      group: "typescript",
    },
    {
      id: "ffffffff-ffff-4fff-8fff-ffffffffffff",
      stable_identity: "rust:src/isolated.rs:function:IsolatedHelper:50-60",
      label: "IsolatedHelper",
      node_kind: "code_symbol",
      language: "rust",
      symbol_kind: "function",
      file_path: "src/isolated.rs",
      name: "IsolatedHelper",
      qualified_name: "IsolatedHelper",
      start_line: 50,
      end_line: 60,
      degree: 0,
      seed: false,
      group: "rust",
    },
    {
      id: "99999999-9999-4999-8999-999999999999",
      stable_identity: "ts:web/src/GraphDetails.tsx:function:GraphDetails:70-80",
      label: "GraphDetails",
      node_kind: "code_symbol",
      language: "typescript",
      symbol_kind: "component",
      file_path: "web/src/GraphDetails.tsx",
      name: "GraphDetails",
      qualified_name: "GraphDetails",
      start_line: 70,
      end_line: 80,
      degree: 1,
      seed: false,
      group: "typescript",
    },
  ],
  edges: [
    {
      id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
      source: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      target: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      edge_kind: "calls",
      reference_kind: "call",
      confidence: 0.95,
      file_path: "src/handler.rs",
      start_line: 15,
      end_line: 15,
      resolution_status: "resolved",
    },
    {
      id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
      source: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      target: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      edge_kind: "references",
      reference_kind: "reference",
      confidence: 0.9,
      file_path: "src/handler.rs",
      start_line: 18,
      end_line: 18,
      resolution_status: "resolved",
    },
    {
      id: "88888888-8888-4888-8888-888888888888",
      source: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      target: "99999999-9999-4999-8999-999999999999",
      edge_kind: "renders",
      reference_kind: "reference",
      confidence: 0.88,
      file_path: "web/src/GraphTab.tsx",
      start_line: 24,
      end_line: 24,
      resolution_status: "resolved",
    },
  ],
};

// Unit tests with a stub fetch — no network. Run: npm test
import assert from "node:assert/strict";
import { test } from "node:test";

import { MemoryLayerClient, MemoryLayerError } from "../src/index.ts";

function stubFetch(responses: Array<{ status: number; body: unknown }>) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const impl = async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    const next = responses.shift();
    if (!next) throw new Error("no stubbed response left");
    return new Response(JSON.stringify(next.body), { status: next.status });
  };
  return { impl, calls };
}

test("query parses the answer and preserves unknown fields", async () => {
  const { impl, calls } = stubFetch([
    {
      status: 200,
      body: {
        answer: "Port 7420.",
        confidence: 0.9,
        insufficient_evidence: false,
        results: [
          {
            memory_id: "m1",
            summary: "Gateway on 7420",
            memory_type: "reference",
            score: 12.5,
            snippet: "…",
            future_field: "ignored",
          },
        ],
        brand_new_top_level_field: true,
      },
    },
  ]);
  const client = new MemoryLayerClient({ fetchImpl: impl, token: "t" });
  const answer = await client.query("demo", "which port?", { deterministic: true });
  assert.equal(answer.answer, "Port 7420.");
  assert.equal(answer.results[0].future_field, "ignored");
  const sent = JSON.parse(String(calls[0].init?.body));
  assert.equal(sent.answer_mode, "deterministic");
});

test("remember captures then curates bounded", async () => {
  const { impl, calls } = stubFetch([
    { status: 200, body: { raw_capture_id: "cap-1" } },
    { status: 200, body: { output_count: 1 } },
  ]);
  const client = new MemoryLayerClient({ fetchImpl: impl, token: "t" });
  const result = await client.remember("demo", { title: "t", summary: "s" });
  assert.equal(result.output_count, 1);
  const curatePayload = JSON.parse(String(calls[1].init?.body));
  assert.equal(curatePayload.raw_capture_id, "cap-1");
});

test("errors raise with status", async () => {
  const { impl } = stubFetch([{ status: 401, body: { error: "invalid api token" } }]);
  const client = new MemoryLayerClient({ fetchImpl: impl, token: "wrong" });
  await assert.rejects(
    () => client.stats(),
    (error: unknown) => error instanceof MemoryLayerError && error.status === 401,
  );
});

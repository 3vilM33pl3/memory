# @memory-layer/client (TypeScript)

Typed, zero-dependency TypeScript client for the [Memory Layer](https://www.memory-layer.dev) HTTP API v1 — the frozen `x-stability: core` surface ([reference](https://www.memory-layer.dev/docs/reference/http-api)). Uses global `fetch` (Node ≥ 18 or any browser).

```ts
import { MemoryLayerClient } from "@memory-layer/client";

const client = new MemoryLayerClient(); // http://127.0.0.1:4040
await client.remember("notes", { title: "Tried the client", summary: "It speaks core v1." });
const answer = await client.query("notes", "What did I try?", { deterministic: true });
console.log(answer.answer, answer.insufficient_evidence);
```

Highlights: `query`/`queryGlobal` (deterministic mode for reproducible keyless answers), `remember` (capture + bounded curate), `memoryGraph` (typed nodes incl. decayed ACT-R `activation`), `resume`, `overview`, `health`/`stats`. Unknown response fields are preserved via index signatures — core v1 is additive-only.

Build `npm run build`; test `npm test` (stub fetch, no network). npm release pending.

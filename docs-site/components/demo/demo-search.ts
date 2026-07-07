import type { DemoMemory } from "./demo-data";

// A miniature of the real deterministic retrieval + answer synthesis, running
// entirely in the browser over the demo snapshot: term-overlap lexical
// ranking, citation of the memories the answer draws from, and an honest
// refusal when nothing anchors a result to the question — the same behavioral
// contract the engine's eval-gated synthesizer follows.

const STOPWORDS = new Set([
  "the", "a", "an", "is", "are", "was", "were", "be", "for", "of", "on", "in",
  "to", "and", "or", "as", "at", "by", "with", "it", "its", "that", "this",
  "how", "what", "which", "does", "do", "did", "when", "where", "why", "who",
  "can", "could", "should", "would", "will", "you", "your", "we", "our",
]);

export interface DemoQueryResult {
  memoryId: string;
  score: number;
  match: string;
  cited: boolean;
}

export interface DemoQueryResponse {
  answer: string;
  confidence: number;
  insufficientEvidence: boolean;
  diagnostics: string[];
  results: DemoQueryResult[];
}

function tokens(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .split(/[^a-z0-9]+/)
      .filter((token) => token.length >= 2 && !STOPWORDS.has(token)),
  );
}

function overlap(queryTokens: Set<string>, memory: DemoMemory): number {
  if (!queryTokens.size) return 0;
  const haystack = tokens(
    `${memory.summary} ${memory.preview} ${memory.canonicalText} ${memory.tags.join(" ")}`,
  );
  let hits = 0;
  for (const token of queryTokens) if (haystack.has(token)) hits += 1;
  return hits / queryTokens.size;
}

function sameTopic(a: DemoMemory, b: DemoMemory): boolean {
  const aTokens = tokens(a.summary);
  const bTokens = tokens(b.summary);
  const smaller = Math.min(aTokens.size, bTokens.size);
  if (!smaller) return false;
  let shared = 0;
  for (const token of aTokens) if (bTokens.has(token)) shared += 1;
  return shared / smaller >= 0.55;
}

export function runDemoQuery(question: string, memories: DemoMemory[]): DemoQueryResponse {
  const queryTokens = tokens(question);
  const scored = memories
    .map((memory) => ({ memory, overlap: overlap(queryTokens, memory) }))
    .filter((entry) => entry.overlap > 0)
    .sort((a, b) => b.overlap - a.overlap)
    .slice(0, 7);

  const top = scored[0];
  // Refusal contract: no anchored top result means no confident answer.
  if (!top || top.overlap < 0.4) {
    return {
      answer: "I could not find enough project memory to answer confidently.",
      confidence: top ? Math.min(0.3, top.overlap) : 0,
      insufficientEvidence: true,
      diagnostics: [`lexical ${scored.length}`, "returned 0", "browser-local search"],
      results: scored.map((entry) => ({
        memoryId: entry.memory.id,
        score: entry.overlap * 40,
        match: "lexical",
        cited: false,
      })),
    };
  }

  // Runner-up joins the answer only when it adds a different topic.
  const runnerUp = scored
    .slice(1)
    .find((entry) => entry.overlap >= top.overlap * 0.72 && !sameTopic(top.memory, entry.memory));
  const answer = runnerUp
    ? `${top.memory.summary} Also relevant: ${runnerUp.memory.summary}.`
    : top.memory.summary;
  const cited = new Set([top.memory.id, ...(runnerUp ? [runnerUp.memory.id] : [])]);

  return {
    answer,
    confidence: Math.min(0.95, 0.4 + top.overlap * 0.5 + (runnerUp ? 0.08 : 0)),
    insufficientEvidence: false,
    diagnostics: [
      `lexical ${scored.length}`,
      `top overlap ${(top.overlap * 100).toFixed(0)}%`,
      `returned ${scored.length}`,
      "browser-local search",
    ],
    results: scored.map((entry) => ({
      memoryId: entry.memory.id,
      score: entry.overlap * 40,
      match: "lexical",
      cited: cited.has(entry.memory.id),
    })),
  };
}

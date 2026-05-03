# Memory Layer Demo Script

This file is both the human script and the machine-readable source for
`demo/render_demo.py`.

## Scene 1: The Re-Entry Problem
Duration: 12
Caption: Coding agents lose useful project context between sessions.
Narration:
Every new coding-agent session starts with a handicap. The repo is still there,
but the useful context is gone: decisions, failed attempts, reviewer feedback,
and why the last agent chose a particular path.
Terminal:
```text
$ codex exec "continue the Memory Layer work"
Reading files...
No prior session context found.
Question: which constraints are still active?
Risk: repeat an old mistake or miss a hidden project rule.
```

## Scene 2: Capture Then Curate
Duration: 14
Caption: Memory Layer separates raw evidence from curated durable memory.
Narration:
Memory Layer treats memory as evidence first. It captures raw task material,
then curates smaller durable facts with confidence, tags, and provenance back
to the task or files that produced them.
Terminal:
```text
$ memory remember finish-task --project memory
Raw capture:
  prompt, summary, changed files, tests, notes
Curation:
  canonical memory, type, tags, confidence, sources
Result:
  durable project memory with an audit trail
```

## Scene 3: Query From The Agent Loop
Duration: 16
Caption: Agents query project memory before acting.
Narration:
The agent-facing path is a normal CLI query. The answer is synthesized from
curated memories, and the output keeps the evidence visible instead of hiding
retrieval behind a black box.
Terminal:
```text
$ memory query --project memory \
  --question "How should graph-aware retrieval be interpreted?"

Answer:
Graph retrieval is additive to lexical and semantic search. It can boost
memories connected to matching symbols or nearby code, but answers still cite
curated memories rather than raw graph rows. [1]

Confidence: 0.82 | Evidence: sufficient | Method: llm | Citations: 1
```

## Scene 4: Provenance Is The Interface
Duration: 14
Caption: Citations and source lines make the answer inspectable.
Narration:
For infrastructure engineers, the important part is not just recall. The agent
can inspect why a memory was returned: cited memories, file provenance, task
evidence, and retrieval diagnostics.
Terminal:
```text
Cited memories:
1. Query uses the latest completed code graph extraction [implementation]

1. Query graph retrieval [implementation / hybrid] score=23.37
  why: semantic similarity 0.53 | relation boost 17.72 | graph match boost
  source: crates/mem-search/src/lib.rs file
  source: docs/user/cli/query.md file
  source: task_prompt note
```

## Scene 5: More Than Vector Search
Duration: 16
Caption: Code graph evidence augments retrieval without replacing curation.
Narration:
That makes this more than vector search. Memory Layer can use parser-backed
code symbols, references, and graph edges as ranking evidence, while keeping
human-curated memory as the citation surface.
Terminal:
```text
$ memory graph status --project memory --text
Code graph status for memory
Status: completed
Symbols: 1842 | References: 6910 | Resolved: 4217
Graph: nodes 1842 | edges 4217 | evidence 8759

Diagnostics: lexical 64 | semantic 49 | graph active
graph: code symbol match crates/mem-cli/src/main.rs symbol=print_query_response
```

## Scene 6: What To Measure Next
Duration: 17
Caption: Future evals should measure re-entry, mistake avoidance, and handoff quality.
Narration:
The next question is measurable behavior. Future evals should compare agents
with and without Memory on re-entry speed, avoided repeated mistakes, and the
quality of handoffs between sessions.
Terminal:
```text
$ memory eval run \
  --suite evals/suites/memory-improvement-v1 \
  --condition no-memory \
  --condition full-memory \
  --repeat 5 --text

Metrics to watch:
  re-entry coverage, mistake avoidance, handoff completeness
  recall@k, citation coverage, token and latency deltas
```

## Scene 7: Closing Claim
Duration: 12
Caption: Durable memory gives agents evidence they can re-enter with.
Narration:
The core idea is simple: do not ask every agent to rediscover the project.
Capture evidence, curate durable memory, preserve provenance, and let the next
agent start from grounded project context.
Terminal:
```text
Memory Layer
  local-first project memory for coding agents
  raw evidence -> curated facts -> cited answers
  provenance, curation, and code graph evidence
```

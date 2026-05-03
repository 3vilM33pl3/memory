# Memory Improvement V1 Full Benchmark Run

Date: 2026-05-03

## Summary

This run executed the Dockerized `memory-improvement-v1` benchmark with five
paired repeats, comparing a `no-memory` baseline against the `full-memory`
condition. The LLM judge was enabled, but deterministic scoring remained the
source of truth for success.

The headline result is positive but specific: Memory strongly improved
retrieval and grounded-answer tasks, and the aggregate paired success rate moved
from `0.0%` to `18.1%`. The run does **not** yet prove that Memory improved
long-running autonomous coding continuity, because the Codex sequence runs did
not produce the required verified Memory query artifacts.

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired comparison units | 155 | 155 | 0 |
| Success rate | 0.0% | 18.1% | +18.1 pp |
| McNemar p-value |  |  | 0.0000 |
| Total tokens | 22,069,461 | 12,970,186 | -9,099,275 |
| Mean duration | 40,501.3 ms | 18,954.2 ms | -21,547.1 ms |
| `recall_at_k` | 0.000 | 1.000 | +1.000 |
| `mrr` | 0.000 | 1.000 | +1.000 |
| `ndcg` | 0.000 | 1.000 | +1.000 |
| `assertion_recall` | 0.000 | 0.725 | +0.725 |

## Run Configuration

Command:

```bash
MEMORY_EVAL_CONDITIONS='no-memory full-memory' \
MEMORY_EVAL_LLM_JUDGE=1 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

The Docker stack started PostgreSQL with pgvector, the Memory service, seeded
benchmark memories, graph extraction, and an evaluation runner. The benchmark
used hidden memory-only facts: the fixture repository did not contain the
remembered release rule, current codename, UI convention, incident diagnosis, or
graph evidence. That matters because the no-memory condition could not recover
the expected facts by simply reading local files.

Artifacts:

- Generated Markdown report:
  `target/memory-evals/memory-improvement-v1-report-20260503062007.md`
- Comparison JSON:
  `target/memory-evals/memory-improvement-v1-comparison-20260503062007.json`
- Run group:
  `1a4b75bf-ca1d-4c73-839e-4936c4c51fc0`
- Suite checksum:
  `05b54f08b1df1be19ad9db260c405384a3422ff777dcf978cb161030254a5ccc`

Completed run artifacts:

| Condition | Repeat | Artifact |
| --- | ---: | --- |
| `no-memory` | 0 | `target/memory-evals/memory-improvement-v1-no-memory-r0-20260503062007.json` |
| `full-memory` | 0 | `target/memory-evals/memory-improvement-v1-full-memory-r0-20260503062525.json` |
| `no-memory` | 1 | `target/memory-evals/memory-improvement-v1-no-memory-r1-20260503063608.json` |
| `full-memory` | 1 | `target/memory-evals/memory-improvement-v1-full-memory-r1-20260503064100.json` |
| `no-memory` | 2 | `target/memory-evals/memory-improvement-v1-no-memory-r2-20260503065255.json` |
| `full-memory` | 2 | `target/memory-evals/memory-improvement-v1-full-memory-r2-20260503065743.json` |
| `no-memory` | 3 | `target/memory-evals/memory-improvement-v1-no-memory-r3-20260503070904.json` |
| `full-memory` | 3 | `target/memory-evals/memory-improvement-v1-full-memory-r3-20260503071336.json` |
| `no-memory` | 4 | `target/memory-evals/memory-improvement-v1-no-memory-r4-20260503072312.json` |
| `full-memory` | 4 | `target/memory-evals/memory-improvement-v1-full-memory-r4-20260503072733.json` |

The run also exposed two Docker harness issues before the successful rerun:
container execution now uses `/usr/local/bin/memory`, built inside the image,
and the benchmark containers run with `MEMORY_LAYER_PROFILE=prod`. Those fixes
avoid host glibc mismatches and accidental dev-overlay config requirements.

## How The Benchmark Works

The benchmark is a paired ablation. Each item is run once without Memory and
once with Memory, then `memory eval compare` pairs the results by item, repeat,
and sequence step. This design is stronger than comparing two unrelated runs
because each candidate result is judged against the same prompt, fixture,
expected facts, and repeat index.

The suite contains four item families:

- `retrieval_qa`: asks whether the Memory service retrieves expected tags and
  files. The no-memory condition has no retrieval channel, so recall metrics are
  expected to be zero there.
- `grounded_answer`: asks a question that needs hidden Memory facts, then checks
  required and forbidden assertions in the answer.
- `resume_quality`: asks for a new-agent briefing and checks whether important
  remembered topics appear without superseded or unsafe topics.
- `agent_build_sequence`: runs a 20-step Codex continuity task in one copied
  app workspace and scores each step using file, content, command, token, and
  Memory-query-evidence checks.

The `full-memory` condition uses the normal combined retrieval path: lexical,
semantic, graph candidates, and relation boosts. The benchmark also records
retrieval diagnostics such as `recall_at_k`, `mrr`, `ndcg`,
`tag_recall_at_k`, and `file_recall_at_k`. For answer-like items, an optional
LLM judge adds diagnostic scores for evidence use, reasoning quality,
consistency, and maintainability. Those judge scores help explain behavior, but
they do not replace deterministic pass/fail checks.

The suite is also tagged with three reasoning modes:

- `deductive`: apply an explicit remembered rule or constraint.
- `inductive`: infer a convention from remembered examples.
- `abductive`: select the best diagnosis or explanation from remembered
  evidence.

This taxonomy follows the framing in Liu, Neubig, and Andreas,
[An Incomplete Loop: Deductive, Inductive, and Abductive Learning in Large
Language Models](https://arxiv.org/abs/2404.03028v1). Their useful point for
this benchmark is that "reasoning" is not one behavior. A memory system might
help explicit rule application while failing convention inference or diagnosis,
so the report groups results by reasoning mode.

The retrieval design is related to Retrieval-Augmented Generation in Lewis et
al.,
[Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks](https://arxiv.org/abs/2005.11401):
instead of relying only on model parameters, the agent receives retrieved
external evidence. Memory Layer extends that idea for software work by storing
project facts with provenance, tags, source files, embeddings, and graph
relationships.

The optional judge follows the broad LLM-as-judge pattern studied by Zheng et
al.,
[Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena](https://arxiv.org/abs/2306.05685).
The report treats judge scores as diagnostics because automatic judges can be
useful but are not a substitute for deterministic checks and artifact review.

The coding-continuity portion is closer in spirit to software-engineering
benchmarks such as SWE-bench, introduced by Jimenez et al.,
[SWE-bench: Can Language Models Resolve Real-World GitHub Issues?](https://arxiv.org/abs/2310.06770).
The important shared principle is artifact-backed evaluation: the agent must
modify a real workspace, and the result is scored through concrete files,
commands, and traces rather than a free-form self-report.

## Results By Capability

| Group | Pairs | No Memory | Full Memory | Delta | Token Delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| `retrieval_qa` | 20 | 0.0% | 100.0% | +100.0 pp | 0 |
| `grounded_answer` | 20 | 0.0% | 40.0% | +40.0 pp | +15,486 |
| `resume_quality` | 10 | 0.0% | 0.0% | +0.0 pp | +62,253 |
| `agent_build_sequence` | 5 | 0.0% | 0.0% | +0.0 pp | -4,588,507 |
| `agent_build_sequence_step` | 100 | 0.0% | 0.0% | +0.0 pp | -4,588,507 |
| `memory_capability:retrieval` | 15 | 0.0% | 100.0% | +100.0 pp | 0 |
| `memory_capability:grounded-answer` | 20 | 0.0% | 25.0% | +25.0 pp | -505,157 |
| `memory_capability:curation` | 10 | 0.0% | 30.0% | +30.0 pp | -181,581 |
| `memory_capability:graph` | 15 | 0.0% | 33.3% | +33.3 pp | -536,868 |
| `memory_capability:resume` | 15 | 0.0% | 0.0% | +0.0 pp | -128,662 |
| `memory_capability:coding-continuity` | 65 | 0.0% | 0.0% | +0.0 pp | -7,299,611 |

The strongest evidence is retrieval. `recall_at_k`, `mrr`, `ndcg`,
`tag_recall_at_k`, and `file_recall_at_k` all improved from `0.000` to `1.000`.
That means the benchmark memories were findable through the Memory service and
the expected tags/files were present in the returned result set.

Grounded answers improved but were not perfect. Across the grounded-answer
items, success moved from `0.0%` to `40.0%`, and assertion recall improved from
`0.000` to `0.725`. That is meaningful evidence that retrieved Memory can
convert hidden project facts into useful answers. It is not yet a claim that
all answer synthesis is reliable: `forbidden_hits` increased to `0.233`, and
`citation_precision` reported `1.000 -> 0.000`. The citation metric needs
follow-up because the full-memory answers often include bracketed citations,
but the comparison says citation precision is zero.

Resume quality did not improve under the current scoring. That means the
up-to-speed path either did not include enough required topics, included
forbidden topics, or the suite expectations are misaligned with what the
deterministic resume path emits. This is a product signal, not a harness
success.

The coding sequence did not pass in either condition. The full-memory side
required 20 Memory queries per run, but `memory_queries_verified` remained
`0.000` and `memory_evidence_ok` regressed from `1.000` to `0.000`. The artifact
notes show missing Memory query status and output files for every sequence
step. Therefore the benchmark currently proves that the service and retrieval
path work, but it does not prove that Codex reliably used Memory during the
20-step build sequence.

## Results By Reasoning Mode

| Reasoning Mode | Pairs | No Memory | Full Memory | Delta | Token Delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| `abductive` | 50 | 0.0% | 20.0% | +20.0 pp | -1,439,473 |
| `deductive` | 70 | 0.0% | 18.6% | +18.6 pp | -6,563,290 |
| `inductive` | 35 | 0.0% | 14.3% | +14.3 pp | -1,096,512 |

The split is useful because it shows the improvement is not isolated to one
kind of question. Memory helped all three reasoning groups, with the largest
success delta in abductive tasks. In practical terms:

- Deductive improvement means explicit remembered rules, such as the release
  gate, can be retrieved and applied.
- Inductive improvement means remembered examples can help infer conventions,
  such as the "paper ledger" and "forest accent" UI language.
- Abductive improvement means remembered incidents can support diagnosis, such
  as mapping an amber retry banner to stale retry state.

The mode-level improvements should still be read through the capability-level
breakdown. Some gains come from retrieval and answer items, while the coding
sequence remains a failure in every reasoning mode.

## Worked Examples

### Explicit Release Rule

Question:

```text
Before a Memory release is described as ready, what exact gate must pass?
```

The no-memory baseline answered:

```text
I don't know from the information provided what exact gate must pass before a Memory release is described as ready.
```

The full-memory condition answered:

```text
A Memory release can be described as ready only after the green gate passes for the paired benchmark. [1]
```

This is the cleanest example of Memory helping deductive reasoning. The answer
requires an explicit project rule that is absent from the fixture repository.
No-memory correctly refuses to invent it. Full-memory retrieves and applies it.

### Prior Incident Diagnosis

Question:

```text
A new agent sees an amber retry banner that never clears. What is the likely remembered cause and fix?
```

The full-memory answer identified stale retry state and said to clear it, but
the item still failed because it missed the required incident identifier
`RIFT-27`. This is a useful partial win: Memory supplied the right class of
diagnosis, but answer synthesis did not preserve all required evidence. That
points to a citation/assertion-preservation problem rather than a pure retrieval
failure.

### UI Convention Inference

Question:

```text
What UI pattern should a new Memory continuity page follow based on prior examples?
```

The full-memory answer included the remembered "paper ledger" and "forest
accent" convention, but it also contained a forbidden phrase, "purple default",
while explaining what to avoid. The deterministic scorer treats forbidden
assertion hits literally, so the item failed. That is a scoring-design lesson:
the benchmark should distinguish endorsing a stale/forbidden claim from
mentioning it as something to avoid.

### Coding Sequence Evidence Failure

The 20-step sequence asked Codex to build a continuity app where every
full-memory step had one required Memory question. The full-memory artifacts
showed:

```text
memory_queries_required = 20
memory_queries_verified = 0
memory_evidence_ok = false
```

That result invalidates any claim that this run proves Memory-assisted coding
continuity. The correct interpretation is narrower: the sequence harness ran,
token usage was captured, and failures were detected, but the agent did not
produce the required helper artifacts.

## What The Results Mean

This run supports these claims:

- Memory retrieval is active and reliable for this reviewed benchmark suite.
- Memory can answer hidden project-fact questions better than a no-memory LLM
  baseline.
- The benchmark can compare repeated runs statistically and report token and
  latency cost.
- The benchmark can detect when Memory evidence is missing instead of accepting
  a self-reported agent answer.

This run does not support these claims:

- It does not prove Memory improves arbitrary coding-agent performance.
- It does not prove the 20-step Codex continuity task works with Memory.
- It does not prove publication-grade improvement; the suite is still small and
  project-specific.
- It does not prove LLM judge scores are objective quality scores.

The token result is notable. Full-memory used fewer total tokens than
no-memory: `12,970,186` vs `22,069,461`. That is counterintuitive because
Memory often adds context. In this run, the no-memory build sequence spent many
more tokens despite failing, while the full-memory sequence also failed earlier
or more tersely. The right interpretation is not "Memory is always cheaper";
it is "token cost must be measured, because the direction can depend on task
shape and failure mode."

## Statistical Interpretation

The comparison reports `155` paired units because the sequence contributes both
top-level sequence items and step-level sub-results across five repeats, in
addition to retrieval, answer, and resume items. The McNemar p-value is
`7.45e-9`, rendered as `0.0000`, which says the binary success changes are very
unlikely to be symmetric under the null hypothesis of no condition effect.

That p-value should not be over-read. It is valid for this benchmark's paired
units, but those units are not independent samples from all possible software
engineering tasks. Several paired units come from the same suite, same hidden
fact set, and same 20-step sequence. The result is strong internal evidence for
this benchmark, not an external generalization by itself.

Bootstrap confidence intervals reinforce the specific strengths:

| Metric | No Memory | Full Memory | Delta | 95% CI |
| --- | ---: | ---: | ---: | ---: |
| `recall_at_k` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `mrr` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `ndcg` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `assertion_recall` | 0.000 | 0.725 | +0.725 | +0.675..+0.750 |
| `confidence` | 0.156 | 0.974 | +0.818 | +0.811..+0.826 |
| `memory_queries_verified` | 0.000 | 0.000 | +0.000 | +0.000..+0.000 |

The final row is the key caveat. Memory-backed coding prompts required helper
queries, but verified query count did not improve at all.

## LLM Judge Diagnostics

With `MEMORY_EVAL_LLM_JUDGE=1`, the run added judge metrics to answer-like
items:

| Judge Metric | No Memory | Full Memory | Delta | 95% CI |
| --- | ---: | ---: | ---: | ---: |
| `judge_evidence_use` | 0.559 | 0.665 | +0.106 | -0.051..+0.268 |
| `judge_reasoning_quality` | 0.592 | 0.648 | +0.056 | -0.072..+0.190 |
| `judge_consistency` | 0.940 | 0.849 | -0.091 | -0.185..-0.015 |
| `judge_maintainability` | 0.862 | 0.745 | -0.117 | -0.202..-0.041 |

These scores are mixed. Evidence use and reasoning quality moved up, but the
confidence intervals overlap zero. Consistency and maintainability moved down.
That matches the qualitative examples: Memory often provided useful evidence,
but answer synthesis sometimes omitted required identifiers, mentioned
forbidden text, or failed citation precision checks.

## Follow-Up Work

- Fix the Codex sequence path so full-memory steps reliably run the generated
  Memory helper and write verified query artifacts.
- Rerun `memory-improvement-v1` after the sequence fix and compare the new
  `memory_queries_verified`, `memory_evidence_ok`, and step success rates.
- Investigate `citation_precision` reporting for full-memory answers that
  contain bracketed citations.
- Refine forbidden-assertion scoring so "do not use X" is not treated the same
  as endorsing `X`.
- Improve resume scoring or resume output so required project topics can be
  traced and measured.
- Expand the reviewed held-out suite before making broad public claims.
- Preserve official artifacts with release notes, because `target/` is a local
  build-artifact directory and can be cleaned.

## Bottom Line

This is the strongest benchmark run so far for Memory's retrieval and
grounded-answer value. It shows a statistically significant improvement on the
reviewed `memory-improvement-v1` suite, with perfect retrieval metrics and a
large answer assertion-recall gain.

It is also useful because it prevented an overclaim. The same report shows that
the long-running coding-continuity path has not yet met the Memory-evidence
standard. The next engineering target is therefore clear: make Codex actually
produce verified Memory query artifacts during the sequence, then rerun the
same benchmark.

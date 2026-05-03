# Memory Improvement V1 Partial Benchmark Run

Date: 2026-05-03

Status: latest local one-repeat partial run. This report is narrower than the
same-day five-repeat full run because it compares only repeat `0` for the
`no-memory` and `full-memory` conditions. It should be used for regression
diagnosis and harness follow-up, not as the headline benchmark claim.

## Summary

This run executed the Dockerized `memory-improvement-v1` benchmark for a
single paired repeat, comparing a `no-memory` baseline against the
`full-memory` condition. The LLM judge was enabled, but deterministic scoring
remained the source of truth for success.

The headline result is still positive but weaker than the five-repeat run:
Memory preserved perfect retrieval, improved hidden-fact grounded answers, and
raised aggregate paired success from `3.2%` to `19.4%`. The run is not
statistically decisive at this size. It also continues to show the same major
coding-continuity failure: full-memory sequence steps required Memory queries,
but none of those query artifacts were verified.

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired comparison units | 31 | 31 | 0 |
| Success rate | 3.2% | 19.4% | +16.1 pp |
| McNemar p-value |  |  | 0.1250 |
| Total tokens | 23,992,517 | 20,766,815 | -3,225,702 |
| Mean duration | 74,343.7 ms | 68,182.6 ms | -6,161.2 ms |
| `recall_at_k` | 0.000 | 1.000 | +1.000 |
| `mrr` | 0.000 | 1.000 | +1.000 |
| `ndcg` | 0.000 | 1.000 | +1.000 |
| `assertion_recall` | 0.000 | 0.625 | +0.625 |

## Run Configuration

Command:

```bash
MEMORY_EVAL_CONDITIONS='no-memory lexical semantic graph full-memory' \
MEMORY_EVAL_LLM_JUDGE=1 \
docker compose -f evals/docker/memory-improvement/compose.yml run --rm eval
```

The generated comparison used the `no-memory` and `full-memory` repeat `0`
artifacts. The same run group also contains `lexical`, `semantic`, and `graph`
condition artifacts, plus a later `no-memory` repeat `1` artifact. Those
additional artifacts are useful for follow-up analysis, but they are not part
of the generated `full-memory` versus `no-memory` comparison summarized here.

Artifacts:

- Generated Markdown report:
  `target/memory-evals/memory-improvement-partial-r0-report-20260503102335.md`
- Comparison JSON:
  `target/memory-evals/memory-improvement-partial-r0-comparison-20260503102335.json`
- Run group:
  `61c49ebf-d8da-4cad-9a25-218d493781a0`
- Suite checksum:
  `05b54f08b1df1be19ad9db260c405384a3422ff777dcf978cb161030254a5ccc`

Completed run artifacts used in this comparison:

| Condition | Repeat | Artifact |
| --- | ---: | --- |
| `no-memory` | 0 | `target/memory-evals/memory-improvement-v1-no-memory-r0-20260503102335.json` |
| `full-memory` | 0 | `target/memory-evals/memory-improvement-v1-full-memory-r0-20260503111042.json` |

Additional same-run-group artifacts not included in the generated comparison:

| Condition | Repeat | Artifact |
| --- | ---: | --- |
| `lexical` | 0 | `target/memory-evals/memory-improvement-v1-lexical-r0-20260503103159.json` |
| `semantic` | 0 | `target/memory-evals/memory-improvement-v1-semantic-r0-20260503104803.json` |
| `graph` | 0 | `target/memory-evals/memory-improvement-v1-graph-r0-20260503105403.json` |
| `no-memory` | 1 | `target/memory-evals/memory-improvement-v1-no-memory-r1-20260503113230.json` |

Do not merge this partial batch into the earlier five-repeat batch when making
token-cost or quality claims. The two reports use the same suite checksum, but
they are different run groups and different repeat sets.

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
| `retrieval_qa` | 4 | 0.0% | 100.0% | +100.0 pp | 0 |
| `grounded_answer` | 4 | 0.0% | 50.0% | +50.0 pp | +3,109 |
| `resume_quality` | 2 | 0.0% | 0.0% | +0.0 pp | +6,717 |
| `agent_build_sequence` | 1 | 0.0% | 0.0% | +0.0 pp | -1,617,764 |
| `agent_build_sequence_step` | 20 | 5.0% | 0.0% | -5.0 pp | -1,617,764 |
| `memory_capability:retrieval` | 3 | 0.0% | 100.0% | +100.0 pp | 0 |
| `memory_capability:grounded-answer` | 4 | 0.0% | 25.0% | +25.0 pp | +1,264,498 |
| `memory_capability:curation` | 2 | 0.0% | 50.0% | +50.0 pp | +3,136,883 |
| `memory_capability:graph` | 3 | 0.0% | 33.3% | +33.3 pp | +1,706,729 |
| `memory_capability:resume` | 3 | 0.0% | 0.0% | +0.0 pp | -1,263,936 |
| `memory_capability:coding-continuity` | 13 | 7.7% | 0.0% | -7.7 pp | -6,127,303 |
| `memory_capability:evaluation` | 3 | 0.0% | 0.0% | +0.0 pp | -1,942,573 |

The strongest evidence is again retrieval. `recall_at_k`, `mrr`, `ndcg`,
`tag_recall_at_k`, and `file_recall_at_k` all improved from `0.000` to
`1.000`. The full-memory condition retrieved the expected facts for all four
retrieval QA items.

Grounded answers improved from `0.0%` to `50.0%`. The full-memory condition
passed the explicit release-gate question and the superseded-codename curation
question. It failed the amber retry diagnosis and UI pattern item even though
retrieval was successful for both. In other words, retrieval found the facts,
but answer synthesis did not preserve enough required assertions in every case.

Resume quality did not pass deterministically in either condition. The
full-memory resume output had substantially better topic recall, moving from
`0.000` to `0.458`, and the judge scored it as coherent and evidence-backed.
The deterministic checks still marked both resume items as failures because
required topics were missing.

The coding sequence regressed relative to the no-memory baseline on step-level
success. No-memory passed one of twenty score commands and preserved all
required files. Full-memory passed zero score commands, had fewer required
files present, and wrote no valid Memory query artifacts. The aggregate
full-memory token count was lower, but that is a failure-mode measurement, not
evidence that Memory made the task cheaper in a useful way.

## Results By Reasoning Mode

| Reasoning Mode | Pairs | No Memory | Full Memory | Delta | Token Delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| `abductive` | 10 | 0.0% | 20.0% | +20.0 pp | +1,038,178 |
| `deductive` | 14 | 0.0% | 21.4% | +21.4 pp | -1,253,708 |
| `inductive` | 7 | 14.3% | 14.3% | +0.0 pp | -3,010,172 |

The reasoning-mode split shows the same broad pattern as the five-repeat run,
but with less stability. Deductive and abductive groups improved on binary
success. Inductive success did not improve because the full-memory UI-pattern
answer mentioned a forbidden stale phrase while the sequence also failed.

In practical terms:

- Deductive improvement means the explicit release rule and current codename
  can be retrieved and applied in answer tasks.
- Abductive improvement means Memory can retrieve incident evidence, but the
  diagnosis answer still needs better assertion preservation.
- Inductive results are mixed: Memory retrieved the design convention, but the
  answer included forbidden text while describing what to avoid.

The coding-continuity caveat affects every reasoning mode. The sequence remains
an artifact-backed failure, so these mode-level improvements mostly reflect
retrieval and short answer tasks.

## Worked Examples

### Explicit Release Rule

Question:

```text
Before a Memory release is described as ready, what exact gate must pass?
```

The no-memory baseline answered:

```text
I don't know from the provided context what exact gate must pass before a Memory release is described as ready.
```

The full-memory condition answered:

```text
A Memory release can be described as ready only after the green gate passes for the paired benchmark. [1]
```

This remains the cleanest deductive win. The required release gate is a hidden
Memory fact, not present in the local fixture. No-memory correctly refused to
invent it. Full-memory retrieved and applied it, with `assertion_recall = 1.0`.

### Prior Incident Diagnosis

Question:

```text
A new agent sees an amber retry banner that never clears. What is the likely remembered cause and fix?
```

The full-memory condition answered:

```text
The likely remembered cause is stale retry state. The fix is to clear the stale retry state before re-running the status poll; the memory also warns not to delete the data. [1]
```

This answer retrieved the right general incident class, but the deterministic
item still failed with `assertion_recall = 0.0`. The likely issue is assertion
specificity: the answer did not preserve the required remembered identifier or
the exact expected details. That is an answer-synthesis failure, not a
retrieval failure, because `recall_at_k`, `mrr`, `ndcg`, and file/tag recall
were all `1.0`.

### UI Convention Inference

Question:

```text
What UI pattern should a new Memory continuity page follow based on prior examples?
```

The full-memory answer included the remembered "paper-ledger" and "forest
accents" convention, but it also said to avoid "the purple default styling."
The deterministic scorer records literal forbidden assertion hits, so the item
failed with `assertion_recall = 0.5` and `forbidden_hits = 1.0`.

That reproduces the same scoring-design lesson from the five-repeat report:
the benchmark should distinguish endorsing a stale or forbidden phrase from
mentioning it as something to avoid.

### Resume Quality Partial Progress

The full-memory resume output included recent work and useful memories such as
the paper-ledger/forest-accent convention, the local-only continuity fixture,
and accessibility requirements. The LLM judge rated the resume outputs highly:
`judge_evidence_use` improved from `0.490` to `0.980`, and
`judge_reasoning_quality` improved from `0.460` to `0.935`.

The deterministic scorer still failed both resume items. Topic recall improved
from `0.000` to `0.458`, but the output did not cover enough required topics.
This is useful product feedback: the resume path is directionally better with
Memory, but the output contract and scoring expectations are not yet aligned.

### Coding Sequence Evidence Failure

The 20-step sequence asked Codex to build a continuity app where every
full-memory step had one required Memory question. The full-memory artifact
showed:

```text
memory_queries_required = 20
memory_queries_verified = 0
memory_evidence_ok = false
```

Every full-memory step reported a non-zero Memory query exit and an invalid
`.memory-eval/q1.json` output. The top-level sequence artifact preserved only
`42` of `44` required files, passed `5` of `63` content assertions, and passed
`0` of `20` score commands.

This invalidates any claim that this partial run proves Memory-assisted coding
continuity. It does prove that the harness is detecting missing or invalid
Memory query artifacts instead of accepting a self-report.

## What The Results Mean

This run supports these claims:

- Memory retrieval remained active and reliable in the latest one-repeat batch.
- Memory improved short hidden-fact grounded answers in this batch.
- The benchmark can expose assertion-preservation failures even after retrieval
  succeeds.
- The sequence scorer can detect invalid Memory query artifacts step by step.

This run does not support these claims:

- It does not supersede the five-repeat full benchmark as the headline result.
- It does not prove a statistically decisive improvement by itself.
- It does not prove Memory improves long-running Codex continuity.
- It does not prove LLM judge scores should replace deterministic checks.

The token result is again notable but should be read narrowly. Full-memory used
fewer total tokens than no-memory: `20,766,815` vs `23,992,517`. Most of that
difference came from the coding sequence, where both conditions failed and
full-memory produced less useful output. The right interpretation is still
"measure token cost per task shape and failure mode," not "Memory is always
cheaper."

## Statistical Interpretation

The comparison reports `31` paired units because the single sequence
contributes both one top-level sequence item and twenty step-level sub-results,
in addition to retrieval, answer, and resume items. The McNemar p-value is
`0.1250`, which is not statistically decisive at conventional thresholds.

That is expected for a one-repeat partial batch. The direction of the aggregate
success delta matches the earlier five-repeat run, but this report should be
treated as a diagnostic follow-up rather than an independent proof point.

Bootstrap confidence intervals are degenerate for several metrics because this
batch has only one repeat:

| Metric | No Memory | Full Memory | Delta | 95% CI |
| --- | ---: | ---: | ---: | ---: |
| `recall_at_k` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `mrr` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `ndcg` | 0.000 | 1.000 | +1.000 | +1.000..+1.000 |
| `assertion_recall` | 0.000 | 0.625 | +0.625 | +0.625..+0.625 |
| `confidence` | 0.150 | 0.973 | +0.823 | +0.823..+0.823 |
| `memory_queries_verified` | 0.000 | 0.000 | +0.000 | +0.000..+0.000 |

The final row is still the key engineering caveat. Full-memory coding prompts
required helper queries, but verified query count did not improve at all.

## LLM Judge Diagnostics

With `MEMORY_EVAL_LLM_JUDGE=1`, the run added judge metrics to answer-like
items:

| Judge Metric | No Memory | Full Memory | Delta | 95% CI |
| --- | ---: | ---: | ---: | ---: |
| `judge_evidence_use` | 0.538 | 0.670 | +0.132 | -0.063..+0.415 |
| `judge_reasoning_quality` | 0.553 | 0.658 | +0.105 | -0.107..+0.365 |
| `judge_consistency` | 0.953 | 0.822 | -0.132 | -0.367..+0.005 |
| `judge_maintainability` | 0.873 | 0.727 | -0.147 | -0.322..-0.012 |

These diagnostics are mixed. Resume quality improved sharply by judge scores,
but grounded-answer judge scores were lower for some full-memory answers that
were terse, under-explained, or failed to preserve required evidence. The
deterministic checks are therefore still doing useful work: they identify exact
missing assertions, forbidden hits, and invalid query artifacts that a holistic
judge score can blur.

## Follow-Up Work

- Fix the full-memory Codex sequence path so generated Memory helper queries
  exit successfully and write valid `.memory-eval/q1.json` artifacts.
- Rerun at least five paired repeats after the sequence fix before updating the
  headline benchmark claim.
- Compare the `lexical`, `semantic`, `graph`, and `full-memory` artifacts from
  run group `61c49ebf-d8da-4cad-9a25-218d493781a0` to isolate which retrieval
  channel contributes most to answer success and sequence failures.
- Investigate why the amber retry answer retrieved the right memory but scored
  `assertion_recall = 0.0`.
- Refine forbidden-assertion scoring so "avoid X" is not treated the same as
  endorsing `X`.
- Align resume output with deterministic topic expectations, or revise the
  scorer if the current expected topics are too rigid.
- Preserve official artifacts with release notes, because `target/` is a local
  build-artifact directory and can be cleaned.

## Bottom Line

The latest one-repeat partial run confirms the same shape as the full
benchmark: Memory retrieval is working, short hidden-fact answers improve, and
resume outputs are directionally better but still miss deterministic targets.

It also confirms the same blocker. The long-running coding-continuity path does
not yet produce verified Memory query artifacts, and full-memory sequence
success remains zero. The next engineering target is unchanged: make Codex
produce valid Memory evidence during the sequence, then rerun a full paired
batch.

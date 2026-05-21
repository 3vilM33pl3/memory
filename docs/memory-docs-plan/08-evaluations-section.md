# Evaluations Section

## Purpose

Make Memory Layer’s scientific/evaluation story visible and credible. This section should communicate that Memory Layer is built to be measured, not merely demoed.

## Section Navigation

```text
Evaluations
  Overview
  Ablation tests
  Run evaluations
  Benchmark reports
  Metrics
  Reproducibility
  Interpreting results
  Limitations
```

## Evaluations Overview Page

Start with the thesis:

> Memory systems should be evaluated by the behaviour they improve. Memory Layer includes a repeatable evaluation harness for testing whether memory changes agent outcomes, retrieval quality, cost, and latency.

Cards:

```text
Run an ablation
Compare no-memory and full-memory variants.

Read a benchmark report
Understand result tables, artifacts, and gates.

Understand metrics
Learn Recall@K, MRR, nDCG, assertion recall, token cost, and latency.

Reproduce results
Use the reproducibility checklist.
```

## Ablation Tests Page

Explain ablation testing in plain language: compare a baseline without memory to a variant with memory, keep everything else as similar as possible, run paired tasks, compare outcomes item-by-item, record artifacts, and interpret cautiously.

Memory-specific variants:

```text
no-memory
full-memory
keyword-only
vector-only
graph-enabled
curation-disabled
stale-memory-included
```

Potential future variants:

```text
human-curated vs auto-curated
single-embedding vs multi-embedding
with-code-graph vs without-code-graph
briefing-only vs query-on-demand
```

## Run Evaluations Page

Document prerequisites, environment, dataset location, running one benchmark, paired repeats, output directory, artifacts, and cleanup.

Use placeholders where exact commands need confirming. Example shape:

```bash
memory eval run --suite memory-improvement-v1 --variant no-memory
memory eval run --suite memory-improvement-v1 --variant full-memory
memory eval compare --suite memory-improvement-v1
```

## Benchmark Reports Page

Explain report anatomy: date, Git commit, suite, variants, repeats, dataset, metrics, gates, artifacts, interpretation, and limits.

## Metrics Page

Include a glossary:

- **Success rate**: whether the task met its expected outcome.
- **Recall@K**: whether relevant items appear in the top K retrieved results.
- **MRR**: how early the first relevant result appears.
- **nDCG**: whether highly relevant results rank near the top.
- **Assertion recall**: whether expected factual assertions were recovered.
- **Token cost**: how much model context was consumed.
- **Latency**: how long retrieval/evaluation took.

## Reproducibility Page

Checklist:

- Commit hash recorded.
- Dataset version recorded.
- Model/provider recorded.
- Configuration recorded.
- Random seeds recorded where applicable.
- Artifacts stored immutably.
- Paired variants compared item-by-item.
- Raw outputs retained.
- Report generated from artifacts, not manual copy/paste.

## Interpreting Results Page

Use careful language. Retrieval success is not automatically autonomous coding success. Token reductions should be interpreted alongside quality. A hidden-fact task supports recall claims, not universal agent-improvement claims.

## Limitations Page

Cover task-suite bias, model/provider variation, stale or wrong memories, curation quality, and the need for real-world longitudinal tests.

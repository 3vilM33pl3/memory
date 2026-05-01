# Eval No-Memory Baseline Plan

## Goal

Add a real `no-memory` LLM baseline to `memory eval`, then tighten condition
isolation enough that eval results are meaningful for Memory Layer versus plain
LLM comparisons.

## Checklist

- [x] Add a direct no-memory LLM path in `memory eval run` for answer and resume eval items.
- [x] Capture latency and provider token usage for direct no-memory LLM calls.
- [x] Keep HTTP/provider execution in `mem-cli` and scoring/statistics in `mem-eval`.
- [x] Add result metadata or notes distinguishing plain LLM, Memory-backed LLM, deterministic, and skipped behavior.
- [x] Preserve current artifact compatibility unless a schema change is necessary.
- [x] Make the resume-quality behavior explicit and keep the default Memory condition deterministic.
- [x] Add tests for no-memory result construction and reusable LLM response parsing.
- [x] Update user/developer docs and the example eval suite.
- [x] Run relevant formatting and tests.

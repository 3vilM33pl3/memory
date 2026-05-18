# Eval — External Retriever Command

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `11-eval-safety-and-research-extensibility.md` landed partially. The safety gate (`--allow-shell`) shipped well (`crates/mem-cli/src/main.rs:10540`, `:10725`, tests at `:14789`, docs at `docs/user/cli/eval.md:48`). The second part of the plan — the external-retriever interface — was deferred and never landed.

This blocks the data-scientist/ML-researcher persona identified in the original 2026-05-16 review §6.3, which expected: *"A `memory eval baseline --retriever=<external-cmd>` that lets them benchmark their retriever against Memory Layer's, on Memory's suite."*

## Goal

Land the external-retriever extension point so a researcher can compare *their* retriever against Memory Layer on the existing eval suite without forking.

## PR Shape

One PR. New CLI flag, new plumbing in the eval runner, new docs section, new test.

## Implementation Notes

- New flag: `memory eval --retriever-cmd <executable>` (mutually exclusive with the default in-process retriever).
- Contract: for each eval item, Memory Layer spawns the executable, writes a JSON envelope to stdin:
  ```json
  {
    "schema_version": 1,
    "project": "<slug>",
    "query": "<question>",
    "context": { "fixture_path": "<absolute-path>", "hidden_facts": [...] },
    "limit": 10
  }
  ```
  Memory Layer reads a JSON envelope back from stdout:
  ```json
  {
    "schema_version": 1,
    "results": [
      { "id": "<external id>", "score": 0.82, "text": "...", "citations": [...] }
    ],
    "diagnostics": { "latency_ms": 123, "tokens_in": 50, "tokens_out": 20 }
  }
  ```
- Reuse the same scoring pipeline that scores in-process results today (deterministic + optional LLM judge). The retriever is pluggable; the scorer is fixed.
- Reuse the `--allow-shell` gate from plan `11` — `--retriever-cmd` implies shell trust and must be opt-in. Document this loudly in the eval docs.
- Document the contract in `docs/user/cli/eval.md` with one full request/response example and one minimal reference retriever (10-line Python `cat`/`echo` example).
- Out of scope: writing a real reference retriever; the audit asks for the *interface*, not a sample implementation.

## Tests

- `cargo test -p mem-cli --all-targets --locked`.
- Unit test: a fake retriever (small Rust binary built in `tests/`) that returns canned JSON; assert Memory Layer parses and scores correctly.
- Integration test: run the existing `memory-improvement-v1` suite once with the in-process retriever and once with the fake retriever; assert both produce comparable artifact shapes.
- Negative test: `--retriever-cmd` without `--allow-shell` fails with the same error pattern the safety gate already uses.

## Acceptance Criteria

- A researcher can run `memory eval --retriever-cmd ./my-retriever --allow-shell --suite memory-improvement-v1` and get a scored artifact identical in shape to the in-process artifact.
- Without `--allow-shell`, the flag is rejected with the existing gate's error message — no silent shell execution.
- The docs include the JSON contract, one full example, and a copy-pasteable minimal retriever stub.
- No change to the default in-process retriever path or its outputs.

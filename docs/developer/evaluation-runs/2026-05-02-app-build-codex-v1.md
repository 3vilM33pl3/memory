# App Build Codex v1 Evaluation Run (Invalidated)

Date: 2026-05-02

## Summary

Status: invalidated for Memory-quality claims.

This run exercised the real Codex-backed app build simulation suite:
`evals/suites/app-build-codex-v1`.

The suite compares the same three static app build tasks under two conditions:

- `no-memory`: Codex is explicitly forbidden from using Memory and must not
  create `memory-evidence.md`.
- `full-memory`: Codex receives required Memory questions, the
  `$MEMORY_EVAL_MEMORY_COMMAND` helper, and must create `memory-evidence.md`.

Final result: both conditions passed all three deterministic build checks, but
the result must not be used as evidence that Memory improved Codex output.

This validates the real build-simulation harness and artifact flow. It does not
yet show a quality advantage for Memory, because both conditions reached the
same score and the Codex subprocesses reported that direct Memory CLI queries
were unreachable from their sandbox.

The benchmark has since been tightened so `full-memory` items must contain
harness-verifiable `.memory-eval/` query artifacts produced by successful
Memory CLI calls. A hand-written `memory-evidence.md` is no longer sufficient.

See `2026-05-02-app-build-codex-v1-validated.md` for the replacement run.

## Commands

Doctor:

```bash
cargo run --bin memory -- eval doctor \
  --suite evals/suites/app-build-codex-v1 \
  --text
```

Final full-memory rerun:

```bash
MEMORY_EVAL_CODEX_MODEL=gpt-5.4-mini \
MEMORY_EVAL_CODEX_WATCHDOG_SECONDS=240 \
cargo run --bin memory -- eval run \
  --suite evals/suites/app-build-codex-v1 \
  --condition full-memory \
  --profile llm \
  --repeat 1 \
  --text
```

Comparison:

```bash
cargo run --bin memory -- eval compare \
  --baseline target/memory-evals/app-build-codex-v1-no-memory-r0-20260502140926.json \
  --candidate target/memory-evals/app-build-codex-v1-full-memory-r0-20260502143421.json \
  --text
```

## Artifacts

- Baseline run:
  `target/memory-evals/app-build-codex-v1-no-memory-r0-20260502140926.json`
- Candidate run:
  `target/memory-evals/app-build-codex-v1-full-memory-r0-20260502143421.json`
- No-memory workspaces:
  `target/memory-evals/build-runs/app-build-codex-v1-*-no-memory-r0-97d3b4702b904885b81ff1999e2a06a4`
- Full-memory workspaces:
  `target/memory-evals/build-runs/app-build-codex-v1-*-full-memory-r0-bc662e9772fe46218d38871daf33da64`

## Results

| Metric | No Memory | Full Memory | Delta |
| --- | ---: | ---: | ---: |
| Paired items | 3 | 3 | 0 |
| Success rate | 100.0% | 100.0% | +0.0 pp |
| `total_score` | 1.000 | 1.000 | +0.000 |
| Score commands passed | 1.000 | 1.000 | +0.000 |
| Required files present | 3.000 | 3.000 | +0.000 |
| Forbidden files absent | 1.000 | 1.000 | +0.000 |
| Mean duration | 239849.0 ms | 239846.7 ms | -2.3 ms |
| Tokens captured by eval harness | 0 | 0 | 0 |

The eval harness currently records provider token usage only when the underlying
evaluated API returns token metadata. These Codex subprocess runs do not yet
feed Codex token usage back into `EvalItemResult`, so token counts are `0` in
the comparison even though Codex used model tokens.

## Per-Task Outcome

| Task | No Memory | Full Memory | Notes |
| --- | --- | --- | --- |
| `memory-landing-page` | pass | pass | Full-memory run created `memory-evidence.md`. |
| `evaluation-dashboard` | pass | pass | Full-memory run created `memory-evidence.md`. |
| `agent-onboarding-page` | pass | pass | Full-memory run created `memory-evidence.md`. |

## Memory Evidence Caveat

The full-memory workspaces include `memory-evidence.md`, which records the
required Memory questions and the facts used. However, the generated evidence
files state that the direct Memory CLI queries could not reach the local service
from the Codex sandbox. The agents then grounded their work in repository docs
and suite fixtures.

This means the run is useful as a harness validation:

- Codex can be run non-interactively against isolated app fixtures.
- The suite can enforce different `no-memory` and `full-memory` instructions.
- Full-memory runs can be required to leave auditable evidence.
- Deterministic score scripts can verify generated app outputs.

It is not yet a valid statistical claim that Memory improves Codex output on
these app-building tasks.

## Follow-Up

Before using this suite as evidence of Memory quality, fix the sandbox/service
connectivity path so full-memory Codex subprocesses can successfully run:

```bash
$MEMORY_EVAL_MEMORY_COMMAND query \
  --project "$MEMORY_LAYER_PROJECT" \
  --question "<required question>" \
  --json
```

After that, rerun the suite with multiple repeats and store comparison JSON
artifacts next to this report.

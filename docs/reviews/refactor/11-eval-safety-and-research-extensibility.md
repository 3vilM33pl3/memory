# Eval Safety And Research Extensibility

## Review Basis

Claude noted that eval suites can execute shell commands and that researchers may want to compare Memory Layer against external retrievers. The shell behavior is expected for first-party suites but risky for shared third-party suites.

## Goal

Make eval execution safer by default and more useful for researchers without weakening the existing Memory Layer ablation harness.

## PR Shape

First PR: safety documentation and explicit shell opt-in. Later PR: external retriever interface.

## Implementation Notes

- Document that eval suites are code execution inputs, not passive data files.
- Add an `--allow-shell` flag or equivalent trust gate for suites that run arbitrary commands.
- Ensure first-party suites and CI smoke runs pass the trust gate intentionally.
- Add a clear failure message when a suite needs shell execution but the flag is missing.
- Plan external retriever support as a command contract: Memory Layer passes query/context fixture data to an executable and reads structured JSON results.
- Keep external retriever scoring separate from production query code.

## Tests

- Unit-test suite parsing for shell-required tasks.
- CLI-test that shell suites fail without the opt-in and run with it in dry-run/offline mode.
- Keep existing offline eval smoke passing in CI.

## Acceptance Criteria

- Users cannot accidentally run third-party shell suites without explicit consent.
- Documentation makes the trust boundary obvious.
- Researcher extension work has a concrete interface direction.

# CLI Command Split

## Review Basis

Claude identified `crates/mem-cli/src/main.rs` as a major contributor barrier. Current `main` is still over 16k lines and contains command definitions, execution, output formatting, client helpers, wizard logic, eval commands, and tests.

## Goal

Split the CLI into command modules so a reviewer can understand a PR by area instead of scanning one giant file.

## PR Shape

This should be a behavior-preserving refactor. Use several small PRs, each moving one command family.

## Implementation Notes

- Create internal modules under `crates/mem-cli/src/commands/`.
- Move command execution handlers first, leaving the top-level `Command` enum stable until module boundaries settle.
- Start with low-risk groups: `health/status/doctor`, `mcp`, `proposals`, and `bundle`.
- Move shared helpers into `client`, `output`, `project`, and `git` modules only when at least two command modules need them.
- Keep public CLI names, help text, JSON shapes, and exit codes unchanged.
- Avoid creating a new `mem-client` crate in the first pass; extract a crate only after internal boundaries are proven.

## Tests

- Run `cargo test -p mem-cli --all-targets --locked`.
- Run CLI help snapshot-style assertions already present in `mem-cli`.
- Manually inspect `memory --help` and one moved command's `--help`.

## Acceptance Criteria

- `main.rs` loses meaningful size without behavior changes.
- Each moved command family has a clear module owner.
- Review diffs are mostly moves, not mixed with semantic changes.

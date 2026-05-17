# CLI UX And Agent Contract

## Review Basis

Claude praised the agent contract but flagged mixed command naming, overlapping diagnostics, watcher legacy confusion, stdout/stderr concerns, and coarse exit-code behavior. Some specifics have changed: `memory remember` already returns JSON, but the broader output contract still deserves a plan.

## Goal

Make CLI behavior predictable for both humans and agents without breaking existing workflows abruptly.

## PR Shape

Use additive changes first, then deprecations. Do not remove commands until docs and warnings have existed for at least one release.

## Implementation Notes

- Add or strengthen `memory status` as the recommended first diagnostic command, aggregating service health, config, watcher summary, MCP status, and skill bundle status.
- Keep `health`, `doctor`, `stats`, and `service status` as compatibility commands that point users to `memory status` where appropriate.
- Define stdout/stderr rules: machine-readable command output on stdout; warnings and progress on stderr.
- Audit agent-called commands for consistent JSON output and stable error fields.
- Document watcher manager as the preferred path and mark legacy watcher foreground flows as advanced/internal where accurate.
- Add exit-code categories only if they can be implemented consistently; otherwise document current behavior and defer.

## Tests

- Extend help metadata tests for `memory status`.
- Add tests for stdout JSON commands that must not include warnings.
- Run `cargo test -p mem-cli --all-targets --locked`.

## Acceptance Criteria

- A new user or agent has one obvious diagnostic command.
- Existing command names continue to work.
- Docs explain which watcher path is preferred.
- Agent-facing output contracts are documented and test-covered.

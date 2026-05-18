# CLI Command Split — Finish

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `04-cli-command-split.md` landed cosmetically. `crates/mem-cli/src/commands/bundle.rs` and `commands/proposals.rs` were created, but `crates/mem-cli/src/main.rs` grew from 15,636 → 16,644 LOC during the same release window because `memory status` and the provenance verifier landed in `main.rs` instead of in module scaffolds. Two of ~22 command families moved; twenty remain.

## Goal

Finish the split so `main.rs` is small enough for a new contributor to navigate. The scaffolding pattern is already proven by `commands/bundle.rs` and `commands/proposals.rs` — apply it across the remaining command families until `main.rs` hits an explicit budget.

## PR Shape

One PR per command-family group. Behavior-preserving moves only. Suggested group order (low-risk first):

1. `commands/health.rs`, `commands/status.rs`, `commands/doctor.rs` (status/diagnostics)
2. `commands/mcp.rs`
3. `commands/watcher.rs`
4. `commands/service.rs`
5. `commands/wizard.rs`, `commands/init.rs`
6. `commands/commits.rs`, `commands/activities.rs`, `commands/up_to_speed.rs`, `commands/resume.rs`
7. `commands/query.rs`, `commands/remember.rs`, `commands/checkpoint.rs`
8. `commands/eval.rs` (the largest single subsystem still in `main.rs`, ~3k LOC including `--allow-shell` validation at `main.rs:10540` and `:10725`)
9. `commands/graph.rs`, `commands/repo.rs`
10. `commands/verify_provenance.rs` (extract the verifier handler at `main.rs:498`, `:2733`)

Each PR moves *handlers* into the new module; the top-level `Command` enum and arg structs can stay in `main.rs` until module boundaries settle.

## Implementation Notes

- Follow the exact shape of `crates/mem-cli/src/commands/bundle.rs`: one `pub async fn handle(...)` per command, dispatch wired from `main.rs`.
- When two or more command modules need the same helper, lift it into a shared module: `commands/output.rs`, `commands/client.rs`, `commands/project.rs`. Do not pre-create these.
- Preserve all stdout/stderr boundaries, JSON shapes, exit codes, and `--help` text exactly.
- Existing CLI tests in `crates/mem-cli/src/main.rs` should move *with* the code they cover.
- Do not extract a new `mem-client` crate in this pack; that decision belongs in a later PR after internal boundaries stabilize.

## Tests

- `cargo test -p mem-cli --all-targets --locked` after each PR.
- `cargo clippy -p mem-cli --all-targets --locked -- -D warnings`.
- Manual smoke: `memory --help` and `memory <moved-command> --help` for each PR.
- Diff each PR with `git diff --stat` — should be dominated by moves (large `-` in `main.rs` and large `+` in the new module file).

## Acceptance Criteria

- **LOC budget**: `crates/mem-cli/src/main.rs` ≤ **3,000 LOC** after the pack completes (down from 16,644). The audit's "cosmetic" verdict was based on the absence of a numeric target; this plan corrects that.
- Each command family has a module owner under `crates/mem-cli/src/commands/`.
- No PR in this pack changes user-visible CLI behavior, JSON output, or exit codes.
- A reviewer can answer "where does `memory <command>` execute?" in under 30 seconds by reading the module tree.

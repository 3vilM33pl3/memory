# API Module Split

## Review Basis

Claude called `mem-api/src/lib.rs` a kitchen sink containing types, config loading, validation, transport framing, and env-file plumbing.

## Goal

Split `mem-api` internally into coherent modules while keeping the public crate API stable.

## PR Shape

Do this as an internal module refactor first. Do not create `mem-api-types`, `mem-api-config`, or `mem-api-transport` crates until the module split has settled.

## Implementation Notes

- Add modules for `types`, `config`, `validation`, `transport`, `env_file`, and `repo_config`.
- Re-export existing public types from `lib.rs` to avoid downstream churn.
- Move tests with their modules, preserving names where possible.
- Keep serde wire shapes exactly unchanged.
- Keep Cap'n Proto framing unchanged in this plan; replacing it is a later architecture decision.

## Tests

- Run `cargo test -p mem-api --all-targets --locked`.
- Run at least one dependent crate test, recommended `cargo test -p mem-cli -p mem-service --all-targets --locked`.
- Compare generated docs or public re-exports if rustdoc warnings appear.

## Acceptance Criteria

- Downstream crates compile without import churn.
- `lib.rs` becomes an index/re-export file, not the implementation home.
- No config format, request type, or response type changes.

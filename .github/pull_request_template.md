## Summary

-

## Scope

- [ ] One focused behavior or docs change
- [ ] Mechanical moves kept separate from behavior changes
- [ ] User-facing docs updated when commands, config, workflows, or output changed

## Validation

- [ ] `cargo fmt --check`
- [ ] Relevant `cargo test -p ... --all-targets --locked`
- [ ] `npm --prefix web run test` / `npm --prefix web run build` if web changed
- [ ] Pgvector-backed tests if migrations, SQL, graph, curation, or repository queries changed
- [ ] Eval dry run or `--allow-shell` run if eval behavior changed

## Notes

- Migrations:
- Service restart needed:
- Shell-executing evals:

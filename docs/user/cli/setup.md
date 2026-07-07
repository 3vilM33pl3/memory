# `memory setup`

One-step setup: machine and project configuration in a single interactive pass.

```bash
cd /path/to/project
memory setup
```

The flow covers the shared machine configuration (database URL, service URL and API token, optional LLM/embedding providers — all skippable) and, when run inside a repository, the repo-local project setup (`.mem/project.toml` plus the `.agents/` skills directory). Strong defaults; accept them and you are done. Secrets are written outside the repository.

```bash
memory setup --dry-run   # preview every file and service action first
memory setup --project custom-slug
```

`memory setup` replaces the older two-step `memory wizard --global` + `memory wizard` flow. `memory wizard` remains available for granular one-layer reconfiguration.

## Related Docs

- [Wizard Command](wizard.md)
- [Init Command](init.md)
- [Doctor Diagnostics](doctor.md)
- [Demo Command](demo.md)

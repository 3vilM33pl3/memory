# `memory dev`

`memory dev` manages the isolated development-profile overlay used when running Memory Layer from a source checkout.

Use it only when developing Memory Layer itself. Normal users should use `memory wizard --global`, `memory wizard`, and the packaged service commands.

## Common Usage

```bash
memory dev init --copy-from-global
memory dev init --dry-run
memory dev init --no-copy-from-global
```

## What It Creates

`memory dev init` creates the user-local project `config.dev.toml` and dev runtime directory. Dev-profile binaries use separate defaults from installed packages:

| Stack | HTTP | Cap'n Proto TCP |
|---|---|---|
| installed package | `127.0.0.1:4040` | `127.0.0.1:4041` |
| dev/cargo run | `127.0.0.1:4250` | `127.0.0.1:4251` |

`--copy-from-global` copies database, LLM, and embedding endpoints from the global config into the dev overlay so a checkout can run without duplicating secrets by hand.

## Related Docs

- [Dev Stack vs Installed Stack](../../developer/dev-stack.md)
- [Running From Source](../getting-started.md#running-from-source)
- [Service Commands](service.md)

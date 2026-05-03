# `memory service`

Use the `service` commands to enable, start, and inspect the shared backend service installed on your machine.

## Table of Contents

- [What It Controls](#what-it-controls)
- [Common Commands](#common-commands)
- [Platform Notes](#platform-notes)
- [Development And Source Use](#development-and-source-use)

## What It Controls

These commands manage the installed shared `memory service` backend.

They are for the normal packaged setup, not for source-level development runs like `cargo run --bin memory -- service run`, which activate a fully isolated dev profile on different ports. See [Dev Stack vs Installed Stack](../../developer/dev-stack.md) for the full contract.

## Common Commands

Enable or start the service:

```bash
memory service enable
```

Preview the service action without changing the machine:

```bash
memory service enable --dry-run
memory service disable --dry-run
memory service ensure-api-token --rotate-placeholder --dry-run
memory service restart-all --dry-run --json
```

This command also provisions the shared service API token automatically if it is missing or still set to the old development placeholder.

Check service status:

```bash
memory service status
```

Rotate an old placeholder token manually:

```bash
memory service ensure-api-token --rotate-placeholder
```

Restart active services after an install or upgrade:

```bash
memory service restart-all --mark-tui-restart --json
```

`restart-all` restarts only services that are already active or loaded. It does not start services that the user intentionally stopped. On Linux it checks the system backend service and active user watcher services. When run as root during a package install, it also checks logged-in user systemd sessions under `/run/user/*`. On macOS it checks loaded Memory Layer LaunchAgents.

Installers pass `--mark-tui-restart` so a TUI that is already open can show `restart` in red in the bottom status bar. Restarting the TUI is still a user action; the installer does not kill the running terminal UI.

Health checks from the client side:

```bash
memory health
memory doctor
```

## Platform Notes

On macOS, `memory service enable` manages the LaunchAgent.

On Linux, the packaged service is usually managed with:

```bash
sudo systemctl enable --now memory-layer.service
memory service restart-all --mark-tui-restart
```

Use `systemctl` directly only when debugging a specific unit.

## Development And Source Use

When working from source, `cargo run --bin memory -- service run` starts a **dev** backend that is fully isolated from any installed service on the same machine: different HTTP port (`4250` vs `4040`), different Cap'n Proto port (`4251` vs `4041`), and its own Cap'n Proto Unix socket under `<repo>/.mem/runtime/dev/`. The dev profile ignores the installed global config entirely.

Bootstrap is one-time per checkout:

```bash
cargo run --bin memory -- init
cargo run --bin memory -- dev init --copy-from-global
cargo run --bin memory -- service run
```

`--copy-from-global` lifts the database URL and LLM/embedding endpoints out of the installed config into the dev overlay. Without it (and without a TTY) you will need to populate those sections in `.mem/config.dev.toml` by hand.

For the full isolation contract, override flags, verification steps, and troubleshooting, see [Dev Stack vs Installed Stack](../../developer/dev-stack.md).

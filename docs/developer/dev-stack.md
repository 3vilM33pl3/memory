# Dev Stack vs Installed Stack

Memory Layer is designed so you can run a development copy from a `cargo` checkout on the same machine that already has the packaged service installed (Debian, Homebrew, or a user-systemd unit) without any conflict between them.

This page is the canonical reference for that split: how the dev profile activates, what gets isolated, and how to bootstrap a fresh checkout.

## Table of Contents

- [Quickstart](#quickstart)
- [How The Profile Is Detected](#how-the-profile-is-detected)
- [What Is Isolated](#what-is-isolated)
- [What Is Shared](#what-is-shared)
- [Default Endpoints](#default-endpoints)
- [Verifying Isolation](#verifying-isolation)
- [Common Pitfalls](#common-pitfalls)

## Quickstart

From a fresh clone, with the packaged service already installed and configured:

```bash
# 1. Clone and build the frontend (web UI is served by the dev backend too).
git clone https://github.com/3vilM33pl3/memory
cd memory
npm --prefix web ci && npm --prefix web run build

# 2. Bootstrap the repo-local .mem/ base config and the dev overlay.
cargo run --bin memory -- init
cargo run --bin memory -- dev init --copy-from-global

# 3. Run each piece in its own shell. All three target the dev stack.
cargo run --bin memory -- service run            # backend (4250 HTTP, 4251 capnp)
cargo run --bin memory -- watcher manager run    # optional: project watchers
cargo run --bin memory -- tui                    # TUI shows [dev] in the header
```

`memory dev init --copy-from-global` lifts the database URL and LLM/embedding endpoints out of the installed config so the dev stack does not need its own credentials. Without `--copy-from-global` (and without a TTY) the overlay is left without shared credentials and you'll need to fill them in manually.

If you skip `memory init`, `memory dev init` will refuse with a clear message — the overlay layers on top of `.mem/config.toml` and needs that file to exist first.

When the dev stack is no longer needed, just stop the three processes. There is nothing to clean up on the installed side.

## How The Profile Is Detected

Every `memory` binary picks its profile at startup:

| Condition | Profile |
| --- | --- |
| Binary path is under `target/{debug,release}/` next to a `Cargo.toml` | `dev` |
| Anything else (packaged install, `~/.cargo/bin/`, systemd unit) | `prod` |
| `MEMORY_LAYER_PROFILE=dev` or `MEMORY_LAYER_PROFILE=prod` set | overrides both |

Practical consequences:

- `cargo run --bin memory -- ...` is always dev.
- `~/.cargo/bin/memory ...` is prod even though it lives in your home dir.
- Set `MEMORY_LAYER_PROFILE=prod` to force a `target/debug/memory` invocation onto the installed stack (rare; mostly useful when reproducing prod-only bugs against a debug build).

The TUI shows `[dev]` in its header when the profile is dev so you cannot mistake one for the other.

## What Is Isolated

The dev profile reads `.mem/config.toml` and then layers `.mem/config.dev.toml` on top of it. The overlay deliberately diverges on the values that would otherwise collide with the installed stack:

| Setting | Installed default | Dev default |
| --- | --- | --- |
| `service.bind_addr` | `127.0.0.1:4040` | `127.0.0.1:4250` |
| `service.capnp_tcp_addr` | `127.0.0.1:4041` | `127.0.0.1:4251` |
| `service.capnp_unix_socket` | `/tmp/memory-layer.capnp.sock` | `<repo>/.mem/runtime/dev/memory-layer.capnp.sock` |
| `automation.state_file_path` | system path | `<repo>/.mem/runtime/dev/automation-state.json` |
| `automation.audit_log_path` | system path | `<repo>/.mem/runtime/dev/automation.log` |
| `cluster.service_id` | derived from bind addr | `memory-layer-dev` |

Importantly, **the dev profile ignores the global config entirely**. That is what keeps a cargo-run service from silently picking up packaged machine-wide settings. Anything the dev stack needs that's normally global (database URL, LLM/embedding endpoints) must live in the dev overlay, which is why `memory dev init --copy-from-global` exists.

Dev binaries also **advertise themselves with a `-dev` version suffix**. `memory --version`, `mem-service --version`, `memory-watch --version`, the `/healthz` JSON `version` field, the cluster discovery packet, and the TUI version panel all report `0.6.0-dev` rather than `0.6.0` when the profile is dev. That way logs, peer lists, and health checks cannot silently conflate a dev service with an installed one.

## What Is Shared

By default, only the **PostgreSQL database** is shared between dev and installed stacks — and only because `memory dev init --copy-from-global` copies the URL into the overlay. If you want the dev stack on a separate database, edit `.mem/config.dev.toml` to point `[database].url` somewhere else.

Tables that are willingly copied from the global config when you opt in:

- `[database]`
- `[llm]`
- `[embeddings]`
- `[features]`
- `[writer]`

Service endpoint, automation paths, and cluster id are intentionally excluded — those are the values that *must* diverge for isolation to hold.

## Default Endpoints

After `memory dev init` with defaults, ports look like this:

| Stack | HTTP | capnp TCP | capnp Unix socket |
| --- | --- | --- | --- |
| Installed (Debian/Homebrew package) | `127.0.0.1:4040` | `127.0.0.1:4041` | `/tmp/memory-layer.capnp.sock` |
| Dev (cargo-run from repo) | `127.0.0.1:4250` | `127.0.0.1:4251` | `<repo>/.mem/runtime/dev/memory-layer.capnp.sock` |

Override the dev ports at bootstrap time:

```bash
cargo run --bin memory -- dev init \
  --bind-addr 127.0.0.1:4260 \
  --capnp-tcp-addr 127.0.0.1:4261
```

## Verifying Isolation

Four quick checks:

```bash
# 1. The TUI header.
cargo run --bin memory -- tui                 # header reads [dev]

# 2. Version string of each binary.
cargo run --bin memory -- --version           # memory 0.6.0-dev
/usr/bin/memory --version                     # memory 0.6.0

# 3. Health endpoint of each stack (reports "version" with or without -dev).
curl -s http://127.0.0.1:4040/healthz         # installed
curl -s http://127.0.0.1:4250/healthz         # dev

# 4. Doctor reports the active profile and resolved overlay path.
cargo run --bin memory -- doctor
```

`memory doctor` will surface the resolved config and overlay paths under the `config.*` checks.

## Common Pitfalls

**`bind capnp tcp addr: timed out waiting for existing backend to release 127.0.0.1:4251`**
Another dev backend is already running. There is only one dev capnp port per `.mem/`, so two simultaneous `cargo run -- service run` from the same checkout will conflict. Stop the other one or pass `--capnp-tcp-addr` to `dev init` to relocate.

**`dev profile active but <repo>/.mem/config.dev.toml is missing`**
You ran a `target/debug/memory` binary in a checkout that has not been bootstrapped yet. Run `memory init && memory dev init` (or `MEMORY_LAYER_PROFILE=prod` to force the installed stack instead).

**Dev stack reads no LLM key even though the installed stack works**
The dev profile does not fall back to the global config. Either rerun `memory dev init --copy-from-global --force` or add `[llm]`/`[embeddings]` blocks directly to `.mem/config.dev.toml`.

**Two installed services on one machine fighting for `127.0.0.1:4041`**
This happens when both a system-wide (`/etc/memory-layer/`) and a user-level (`~/.config/memory-layer/`) install are enabled. Pick one or give the user-level install its own `capnp_tcp_addr` and `capnp_unix_socket`. This is unrelated to the dev stack.

## Related Docs

- [Getting Started](../user/getting-started.md)
- [Service Commands](../user/cli/service.md)
- [Doctor Diagnostics](../user/cli/doctor.md)
- [Architecture Overview](architecture/overview.md)

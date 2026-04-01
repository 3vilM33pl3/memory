# `mem-cli service`

Use the `service` commands to enable, start, and inspect the shared backend service installed on your machine.

## Table of Contents

- [What It Controls](#what-it-controls)
- [Common Commands](#common-commands)
- [Platform Notes](#platform-notes)
- [Development And Source Use](#development-and-source-use)

## What It Controls

These commands manage the installed shared `mem-service` backend.

They are for the normal packaged setup, not for temporary source-level development runs like:

```bash
cargo run --bin mem-service
```

## Common Commands

Enable or start the service:

```bash
mem-cli service enable
```

Check service status:

```bash
mem-cli service status
```

Health checks from the client side:

```bash
mem-cli health
mem-cli doctor
```

## Platform Notes

On macOS, `mem-cli service enable` manages the LaunchAgent.

On Linux, the packaged service is usually managed with:

```bash
sudo systemctl enable --now memory-layer.service
sudo systemctl restart memory-layer.service
```

Use the system service directly for restart operations during upgrades.

## Development And Source Use

When working from source in this repository, you may run a backend manually instead of using the installed service:

```bash
cargo run --bin mem-service
```

That path is useful for local development, but it is separate from the packaged service-management flow.

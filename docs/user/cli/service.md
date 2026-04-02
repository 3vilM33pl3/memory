# `memory service`

Use the `service` commands to enable, start, and inspect the shared backend service installed on your machine.

## Table of Contents

- [What It Controls](#what-it-controls)
- [Common Commands](#common-commands)
- [Platform Notes](#platform-notes)
- [Development And Source Use](#development-and-source-use)

## What It Controls

These commands manage the installed shared `memory service` backend.

They are for the normal packaged setup, not for temporary source-level development runs like:

```bash
cargo run --bin memory -- service run
```

## Common Commands

Enable or start the service:

```bash
memory service enable
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
sudo systemctl restart memory-layer.service
```

Use the system service directly for restart operations during upgrades.

## Development And Source Use

When working from source in this repository, you may run a backend manually instead of using the installed service:

```bash
cargo run --bin memory -- service run
```

That path is useful for local development, but it is separate from the packaged service-management flow.

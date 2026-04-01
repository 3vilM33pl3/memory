# macOS Packaging

Memory Layer now supports a native macOS service model:

- Homebrew for installation
- `launchd` LaunchAgents for the shared backend and per-project watchers
- `~/Library/Application Support/memory-layer/` for shared config, env, runtime, and logs

## Current layout

- Shared config: `~/Library/Application Support/memory-layer/memory-layer.toml`
- Shared env: `~/Library/Application Support/memory-layer/memory-layer.env`
- LaunchAgents: `~/Library/LaunchAgents/`
- Logs: `~/Library/Application Support/memory-layer/logs/`

## Homebrew formula

The development formula lives in `homebrew/memory-layer.rb`.

Example local install from this checkout:

```bash
brew install --HEAD ./packaging/macos/homebrew/memory-layer.rb
```

After install:

```bash
mem-cli wizard
mem-cli service enable
mem-cli watch enable --project <slug>
```

`mem-cli service enable` provisions the shared service API token automatically if it is missing or still set to the development placeholder.

## Service labels

- Backend: `com.memory-layer.mem-service`
- Watcher: `com.memory-layer.memory-watch.<project>`

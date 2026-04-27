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

## Native .pkg installer

Build a standalone macOS `.pkg` from the repo root:

```bash
./packaging/build-pkg.sh                          # unsigned
./packaging/build-pkg.sh --sign "Developer ID Installer: ..."  # signed
```

The `.pkg` installs to `/usr/local/` and seeds `~/Library/Application Support/memory-layer/` on first run.

Output: `target/memory-layer-<version>-macos.pkg`

## Homebrew formula

The canonical Homebrew formula now lives in `../../Formula/memory-layer.rb`.

Install from the tap:

```bash
brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory
brew install --HEAD 3vilM33pl3/memory/memory-layer
```

After install:

```bash
memory wizard
memory service enable
memory watcher enable --project <slug>
```

`memory service enable` provisions the shared service API token automatically if it is missing or still set to the development placeholder.

## Service labels

- Backend: `com.memory-layer.mem-service`
- Watcher: `com.memory-layer.memory-watch.<project>`

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
./packaging/build-pkg.sh                          # unsigned, native architecture
./packaging/build-pkg.sh --arch x86_64            # Intel package
./packaging/build-pkg.sh --arch aarch64           # Apple Silicon package
./packaging/build-pkg.sh \
  --sign-app "Developer ID Application: ..." \
  --sign-pkg "Developer ID Installer: ..."
./packaging/build-pkg.sh \
  --sign-app "Developer ID Application: ..." \
  --sign-pkg "Developer ID Installer: ..." \
  --notarize --notary-profile "memory-notary"
```

The `.pkg` installs to `/usr/local/` and seeds `~/Library/Application Support/memory-layer/` on first run.

Outputs:

- `target/memory-layer-<version>-macos-x86_64.pkg`
- `target/memory-layer-<version>-macos-aarch64.pkg`

## Official signing and notarization

For a proper public macOS distribution:

- sign the `memory` binary with a `Developer ID Application` certificate
- sign the `.pkg` with a `Developer ID Installer` certificate
- notarize the final `.pkg` with Apple using `notarytool`
- staple the notarization ticket to the `.pkg`

One-time credential setup:

```bash
xcrun notarytool store-credentials "memory-notary" \
  --apple-id "<apple-id>" \
  --team-id "<team-id>" \
  --password "<app-specific-password>"
```

Then build, sign, notarize, and staple in one pass:

```bash
./packaging/build-pkg.sh \
  --sign-app "Developer ID Application: <name> (<team-id>)" \
  --sign-pkg "Developer ID Installer: <name> (<team-id>)" \
  --notarize --notary-profile "memory-notary"
```

Validation:

```bash
pkgutil --check-signature target/memory-layer-<version>-macos-<arch>.pkg
xcrun stapler validate target/memory-layer-<version>-macos-<arch>.pkg
spctl -a -vv -t install target/memory-layer-<version>-macos-<arch>.pkg
codesign --verify --verbose=2 /usr/local/bin/memory
```

## Homebrew formula

The canonical Homebrew formula now lives in `../../Formula/memory-layer.rb`.

Install from the tap:

```bash
brew tap 3vilM33pl3/memory https://github.com/3vilM33pl3/memory
brew install 3vilM33pl3/memory/memory-layer
```

For unreleased `main` branch changes:

```bash
brew install --HEAD 3vilM33pl3/memory/memory-layer
```

Release tags publish `memory-<version>.tar.gz` as the Homebrew source archive.
Refresh `Formula/memory-layer.rb` only after that archive exists on the GitHub
release and its SHA256 has been verified.

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

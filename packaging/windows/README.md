# Windows Packaging

Build a native Windows release from PowerShell on a machine with Rust/MSVC, Node, .NET, and WiX 4.0.5 installed:

```powershell
./packaging/windows/build-windows.ps1
```

Outputs are written to `target/windows/dist/`:

- `memory-layer-<version>-windows-x86_64.zip`
- `memory-layer-<version>-windows-x86_64.msi`
- matching `.sha256` files

The unsigned per-user MSI installs under `%LOCALAPPDATA%\Programs\Memory Layer`, adds its `bin` directory to the user `PATH`, and bundles the web UI, skill templates, README, example config, PowerShell completion, and a localhost PostgreSQL + pgvector Compose template.

The portable ZIP uses the same `bin`/`share` layout. Add the extracted `bin` directory to `PATH` before running `memory`.

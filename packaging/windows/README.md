# Windows Packaging

Build a native Windows release from PowerShell on a machine with Rust/MSVC, Node, .NET, and WiX 4.0.5 installed:

```powershell
./packaging/windows/build-windows.ps1
```

Outputs are written to `target/windows/dist/`:

- `memory-layer-<version>-windows-x86_64.zip`
- `memory-layer-<version>-windows-x86_64.msi`
- matching `.sha256` files

The MSI is unsigned and installs `memory.exe` plus the bundled web UI, skill templates, README, example config, and PowerShell completion under `Program Files\Memory Layer`.

# Shell Completion

Use `memory completion` to generate shell completion scripts from the same command metadata that powers `memory --help`.

Supported shells:

```bash
memory completion bash
memory completion zsh
memory completion fish
```

## Package Installs

Normal package installs generate completion files automatically:

- Debian package: `/usr/share/bash-completion/completions/memory`, `/usr/share/zsh/vendor-completions/_memory`, `/usr/share/fish/vendor_completions.d/memory.fish`
- macOS `.pkg`: `/usr/local/share/bash-completion/completions/memory`, `/usr/local/share/zsh/site-functions/_memory`, `/usr/local/share/fish/vendor_completions.d/memory.fish`
- Homebrew: Homebrew's standard bash, zsh, and fish completion directories
- Local install script: `${XDG_DATA_HOME:-~/.local/share}/bash-completion/completions/memory`, `${XDG_DATA_HOME:-~/.local/share}/zsh/site-functions/_memory`, `${XDG_DATA_HOME:-~/.local/share}/fish/vendor_completions.d/memory.fish`

After installing, open a new shell. Bash and fish usually discover these locations automatically when completion support is enabled. For zsh, make sure the installed directory is in `fpath` before `compinit`.

## Manual Setup

Use manual setup when running a development binary or when your shell does not pick up the package-installed files.

Bash:

```bash
mkdir -p ~/.local/share/bash-completion/completions
memory completion bash > ~/.local/share/bash-completion/completions/memory
```

For the current bash session only:

```bash
source <(memory completion bash)
```

Zsh:

```bash
mkdir -p ~/.zfunc
memory completion zsh > ~/.zfunc/_memory
```

Add this before `compinit` in `~/.zshrc` if `~/.zfunc` is not already in `fpath`:

```zsh
fpath=(~/.zfunc $fpath)
autoload -Uz compinit
compinit
```

Fish:

```fish
mkdir -p ~/.config/fish/completions
memory completion fish > ~/.config/fish/completions/memory.fish
```

## Development Builds

When running from source, generate completions with the development binary:

```bash
cargo run --bin memory -- completion bash
cargo run --bin memory -- completion zsh
cargo run --bin memory -- completion fish
```

The generated scripts include root commands such as `memory service`, `memory watcher`, `memory query`, and nested subcommands such as `memory watcher manager`.

## Related Docs

- [Getting Started](../getting-started.md)
- [Service Commands](service.md)
- [TUI Command](tui.md)

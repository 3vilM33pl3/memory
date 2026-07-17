#!/bin/sh
# Memory Layer one-line installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/3vilM33pl3/memory/main/scripts/install.sh | sh
#
# What it does:
#   - macOS:               installs via the Homebrew tap
#   - Debian/Ubuntu:       downloads the latest amd64 or arm64 .deb release
#   - anything else:       points at the Docker Compose stack or source build
#
# After the binary is installed it checks for a reachable PostgreSQL and, when
# none is found, recommends the bundled `docker compose up` stack instead of
# leaving you with a binary that has nothing to talk to.

set -eu

REPO="3vilM33pl3/memory"
BOLD="$(printf '\033[1m')"; RESET="$(printf '\033[0m')"

say()  { printf '%s\n' "$*"; }
head_line() { printf '\n%s%s%s\n' "$BOLD" "$*" "$RESET"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

next_steps() {
    head_line "Installed. Next steps:"
    say "  1. Start the stack (pick one):"
    say "       docker compose up            # bundled Postgres+pgvector+service (from a repo clone)"
    say "       memory wizard --global       # use your own PostgreSQL + pgvector"
    say "  2. Load a showcase project:  memory demo"
    say "  3. Ask it something:         memory query --project demo --question \"How does reinforcement work?\""
    say ""
    say "Five-minute quickstart: https://www.memory-layer.dev/docs/quickstart"
}

check_postgres() {
    if command -v pg_isready >/dev/null 2>&1 && pg_isready -q >/dev/null 2>&1; then
        say "Found a running PostgreSQL. Remember it needs the pgvector extension"
        say "(run: memory doctor --fix)."
    else
        say "No running PostgreSQL detected. The easiest path is the bundled stack:"
        say "  git clone https://github.com/$REPO && cd memory && docker compose up"
    fi
}

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
Darwin)
    head_line "Installing Memory Layer via Homebrew"
    command -v brew >/dev/null 2>&1 || fail "Homebrew is required on macOS: https://brew.sh"
    brew tap 3vilM33pl3/memory "https://github.com/$REPO" >/dev/null
    brew install 3vilM33pl3/memory/memory-layer
    check_postgres
    next_steps
    ;;
Linux)
    deb_arch=""
    case "$arch" in
        x86_64|amd64) deb_arch="amd64" ;;
        aarch64|arm64) deb_arch="arm64" ;;
    esac

    if command -v dpkg >/dev/null 2>&1 && [ -n "$deb_arch" ]; then
        head_line "Installing Memory Layer from the latest .deb release"
        api="https://api.github.com/repos/$REPO/releases/latest"
        deb_url="$(curl -fsSL "$api" | grep -o "https://[^\"]*_${deb_arch}\\.deb\"" | sed 's/"$//' | head -1)"
        [ -n "$deb_url" ] || fail "could not find a .deb asset in the latest release"
        tmp="$(mktemp -d)"
        trap 'rm -rf "$tmp"' EXIT
        say "Downloading $(basename "$deb_url")..."
        curl -fsSL -o "$tmp/memory-layer.deb" "$deb_url"
        say "Installing (requires sudo)..."
        sudo dpkg -i "$tmp/memory-layer.deb" || {
            say "dpkg reported missing dependencies; fixing with apt..."
            sudo apt-get install -f -y
        }
        check_postgres
        next_steps
    else
        head_line "No prebuilt package for $os/$arch yet"
        if command -v dpkg >/dev/null 2>&1; then
            say "Prebuilt Debian packages are currently published for amd64 and arm64."
            say ""
        fi
        say "Two good options:"
        say ""
        say "  Docker (recommended — nothing else to install):"
        say "    git clone https://github.com/$REPO && cd memory && docker compose up"
        say ""
        say "  Build from source (needs Rust + Node):"
        say "    git clone https://github.com/$REPO && cd memory"
        say "    npm --prefix web ci && npm --prefix web run build"
        say "    cargo install --path crates/mem-cli --bin memory"
        exit 0
    fi
    ;;
*)
    fail "unsupported platform: $os. See https://www.memory-layer.dev/docs/install"
    ;;
esac

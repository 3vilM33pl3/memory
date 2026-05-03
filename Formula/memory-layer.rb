class MemoryLayer < Formula
  desc "Local project memory service and terminal UI for coding agents"
  homepage "https://github.com/3vilM33pl3/memory"
  url "https://github.com/3vilM33pl3/memory/releases/download/v0.8.1/memory-0.8.1.tar.gz"
  sha256 "2f6994e99a27bf818585c713208b236a735af4e68272962e5e5396a95102517d"
  head "https://github.com/3vilM33pl3/memory.git", branch: "main"

  depends_on "node" => :build
  depends_on "rust" => :build

  def install
    system "npm", "--prefix", "web", "ci"
    system "npm", "--prefix", "web", "run", "build"
    system "cargo", "build", "--release", "--locked", "--manifest-path", "Cargo.toml",
           "--package", "mem-cli", "--bin", "memory"

    bin.install "target/release/memory"
    bin.install_symlink "memory" => "mem-cli"
    pkgshare.install ".agents/skills/memory-layer" => "skill-template"
    pkgshare.install "memory-layer.toml.example"
    pkgshare.install "web/dist" => "web"
  end

  def caveats
    <<~EOS
      Shared config:
        ~/Library/Application Support/memory-layer/memory-layer.toml

      Shared env:
        ~/Library/Application Support/memory-layer/memory-layer.env

      First run:
        memory wizard --global
        memory service enable

      To test unreleased changes instead:
        brew reinstall --HEAD 3vilM33pl3/memory/memory-layer

      `memory service enable` provisions the shared service API token
      automatically if it is missing or still set to the development placeholder.

      Optional watcher:
        memory watcher enable --project <slug>
    EOS
  end

  test do
    assert_match "memory", shell_output("#{bin}/memory --help")
    assert_predicate bin/"mem-cli", :exist?
    assert_predicate pkgshare/"skill-template", :directory?
    assert_predicate pkgshare/"web/index.html", :exist?
  end
end

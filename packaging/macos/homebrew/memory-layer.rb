class MemoryLayer < Formula
  desc "Local project memory service and terminal UI for coding agents"
  homepage "https://github.com/3vilM33pl3/memory"
  head "https://github.com/3vilM33pl3/memory.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--locked", "--manifest-path", "Cargo.toml",
           "--bin", "memory"

    bin.install "target/release/memory"
    pkgshare.install ".agents/skills" => "skill-template"
    pkgshare.install "memory-layer.toml.example"
  end

  def caveats
    <<~EOS
      Shared config:
        ~/Library/Application Support/memory-layer/memory-layer.toml

      Shared env:
        ~/Library/Application Support/memory-layer/memory-layer.env

      First run:
        memory wizard
        memory service enable

      `memory service enable` provisions the shared service API token
      automatically if it is missing or still set to the development placeholder.

      Optional watcher:
        memory watcher enable --project <slug>
    EOS
  end

  test do
    assert_match "Usage: memory", shell_output("#{bin}/memory --help")
  end
end

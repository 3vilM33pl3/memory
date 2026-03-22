class MemoryLayer < Formula
  desc "Local project memory service and terminal UI for coding agents"
  homepage "https://github.com/3vilM33pl3/memory"
  head "https://github.com/3vilM33pl3/memory.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--locked", "--manifest-path", "Cargo.toml",
           "--bin", "mem-cli", "--bin", "mem-service", "--bin", "memory-watch"

    bin.install "target/release/mem-cli"
    bin.install "target/release/mem-service"
    bin.install "target/release/memory-watch"
    pkgshare.install ".agents/skills/memory-layer" => "skill-template"
    pkgshare.install "memory-layer.toml.example"
  end

  def caveats
    <<~EOS
      Shared config:
        ~/Library/Application Support/memory-layer/memory-layer.toml

      Shared env:
        ~/Library/Application Support/memory-layer/memory-layer.env

      First run:
        mem-cli wizard
        mem-cli service enable

      Optional watcher:
        mem-cli watch enable --project <slug>
    EOS
  end

  test do
    assert_match "memctl", shell_output("#{bin}/mem-cli --help")
  end
end

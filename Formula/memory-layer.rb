class MemoryLayer < Formula
  desc "Local project memory service and terminal UI for coding agents"
  homepage "https://github.com/3vilM33pl3/memory"
  head "https://github.com/3vilM33pl3/memory.git", branch: "main"

  depends_on "node" => :build
  depends_on "rust" => :build

  def install
    system "npm", "--prefix", "web", "ci"
    system "npm", "--prefix", "web", "run", "build"
    system "cargo", "build", "--release", "--locked", "--manifest-path", "Cargo.toml",
           "--bin", "mem-cli", "--bin", "mem-service", "--bin", "memory-watch"

    bin.install "target/release/mem-cli"
    bin.install "target/release/mem-service"
    bin.install "target/release/memory-watch"
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
        mem-cli wizard --global
        mem-cli service enable

      `mem-cli service enable` provisions the shared service API token
      automatically if it is missing or still set to the development placeholder.

      Optional watcher:
        mem-cli watch enable --project <slug>
    EOS
  end

  test do
    assert_match "mem-cli", shell_output("#{bin}/mem-cli --help")
    assert_predicate pkgshare/"skill-template", :directory?
    assert_predicate pkgshare/"web/index.html", :exist?
  end
end

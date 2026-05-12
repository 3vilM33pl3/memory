class MemoryLayer < Formula
  desc "Local project memory service and terminal UI for coding agents"
  homepage "https://github.com/3vilM33pl3/memory"
  url "https://github.com/3vilM33pl3/memory/releases/download/v0.8.3/memory-0.8.3.tar.gz"
  sha256 "8688bf5754ab71d44ab1a775ccb3ed99995e11219cb738311cc03ca08aa9baa2"
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
    (bash_completion/"memory").write Utils.safe_popen_read(bin/"memory", "completion", "bash")
    (zsh_completion/"_memory").write Utils.safe_popen_read(bin/"memory", "completion", "zsh")
    (fish_completion/"memory.fish").write Utils.safe_popen_read(bin/"memory", "completion", "fish")
    (pkgshare/"skill-template").install Dir[".agents/skills/*"]
    pkgshare.install "memory-layer.toml.example"
    pkgshare.install "web/dist" => "web"
  end

  def post_install
    system bin/"memory", "service", "restart-all", "--mark-tui-restart", "--json"
  rescue
    opoo "Memory Layer installed, but automatic service restart failed. Run `memory service restart-all`."
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
    assert_predicate pkgshare/"skill-template/memory-layer/SKILL.md", :exist?
    assert_predicate pkgshare/"skill-template/memory-query-resume/SKILL.md", :exist?
    assert_predicate pkgshare/"web/index.html", :exist?
    assert_predicate bash_completion/"memory", :exist?
    assert_predicate zsh_completion/"_memory", :exist?
    assert_predicate fish_completion/"memory.fish", :exist?
  end
end

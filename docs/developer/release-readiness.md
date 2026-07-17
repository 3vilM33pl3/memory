# Release Readiness

Use this checklist before tagging a Memory Layer release. The goal is to ship a
clean tag from `main`, publish every supported installer, and leave Homebrew in a
verifiable state.

## Release contract

The v1 line preserves:

- stable documented CLI commands and JSON output for core workflows
- stable global and project config file ownership
- append-only database migrations from the latest published release
- read-only MCP tools, resources, and prompts
- packaged service behavior for Debian, Homebrew, macOS `.pkg`, Windows, and source/dev modes

Experimental or advanced surfaces must be documented as such before release:
loop automation, code graph visualization, browser demo data, and research eval
extensions.

## Pre-tag checklist

1. Start from a clean pushed `main`.
2. Choose the release version. Patch releases should normally increment from the
   latest tag, for example `v1.0.1` after `v1.0.0`.
3. Bump Cargo, web, and docs-site metadata to the chosen version.
4. Confirm `Cargo.lock`, `web/package-lock.json`, and
   `docs-site/package-lock.json` match their package manifests.
5. Run:

   ```bash
   cargo fmt --check
   cargo test --workspace --all-targets --locked
   cargo clippy --workspace --all-targets --locked -- -D warnings
   npm --prefix web test
   npm --prefix web run build
   npm --prefix docs-site run build
   ```

6. Run pgvector-backed database tests when the release touches migrations, SQL,
   graph persistence, curation persistence, or service repository queries:

   ```bash
   export MEMORY_LAYER_TEST_DATABASE_URL=postgres://memory:memory@localhost:5432/memory_test
   export MEMORY_LAYER_TEST_REQUIRE_DB=1
   cargo test -p mem-test-support -p mem-graph -p mem-curate -p mem-search -p mem-service --locked
   ```

7. Run the eval checks relevant to the release:

   ```bash
   memory eval doctor --suite evals/suites/research-v1 --text
   memory eval gate --comparison target/memory-evals/comparison.json --policy evals/gates/research-v1.toml --text
   memory eval doctor --suite evals/suites/memory-quality-v1 --text
   memory eval gate --comparison target/memory-evals/quality-comparison.json --policy evals/gates/memory-quality-v1.toml --text
   ```

8. Build the local Debian package smoke:

   ```bash
   ./packaging/build-deb.sh --arch amd64
   ```

9. Commit and push the release-prep changes before tagging.

## Tag and publish

1. Tag the release from the pushed commit:

   ```bash
   git tag v<version>
   git push origin v<version>
   ```

2. Wait for `.github/workflows/release.yml` to finish.
3. Verify the GitHub Release contains:

   - `memory-layer_<version>_amd64.deb`
   - `memory-layer_<version>_arm64.deb`
   - `memory-layer-<version>-macos-x86_64.pkg`
   - `memory-layer-<version>-macos-aarch64.pkg`
   - `memory-layer-<version>-windows-x86_64.zip`
   - `memory-layer-<version>-windows-x86_64.msi`
   - `memory-<version>.tar.gz`
   - matching `.sha256` files for every artifact

4. Compute or confirm the source archive SHA256 from the published release
   asset, then update `Formula/memory-layer.rb`.
5. Validate the formula:

   ```bash
   ruby -c Formula/memory-layer.rb
   ```

6. Commit and push the Homebrew formula refresh.

## Install smoke

Smoke test at least one fresh install and one upgrade path before announcing the
release:

- Debian amd64: install `memory-layer_<version>_amd64.deb`
- Debian arm64: install `memory-layer_<version>_arm64.deb` on a 64-bit Raspberry Pi or equivalent ARM Linux host
- macOS: install Homebrew or the matching `.pkg`
- Windows: install the x86_64 MSI or unzip the x86_64 ZIP

For each smoke test:

```bash
memory --version
memory doctor
memory health
memory status --project <project-slug>
```

## Known cautions

- Do not edit already-applied database migrations.
- Do not update the Homebrew formula before the release source tarball exists.
- If macOS signing secrets are absent, the release may contain unsigned `.pkg`
  files; note that in release communication.

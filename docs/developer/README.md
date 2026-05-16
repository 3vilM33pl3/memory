# Developer Documentation

This section is for developers working on Memory Layer itself: architecture, internal workflows, packaging, plans, and skill-facing material.

## Table of Contents

- [Quickstart](#quickstart)
- [Architecture](#architecture)
- [Implementation Material](#implementation-material)
- [Related Docs](#related-docs)

## Quickstart

Run a fully isolated dev copy of Memory Layer alongside any packaged install on the same machine:

```bash
cargo run --bin memory -- init
cargo run --bin memory -- dev init --copy-from-global
cargo run --bin memory -- service run            # 4250 HTTP, 4251 capnp
cargo run --bin memory -- tui                    # header reads [dev]
```

The dev stack is detected automatically when the binary path lives under `target/{debug,release}/` and reads its config from the user-local project `config.toml` plus a user-local project `config.dev.toml` overlay — never from the installed global config.

For the full isolation contract, default ports, override flags, and troubleshooting, see [Dev Stack vs Installed Stack](dev-stack.md).

## Architecture

- [Dev Stack vs Installed Stack](dev-stack.md)
- [Architecture Overview](architecture/overview.md)
- [Built-In MCP Server](architecture/mcp-server.md)
- [Memory Types Reference](architecture/memory-types.md)
- [How Memory Layer Works](architecture/how-it-works.md)
- [Embeddings and Search](architecture/embeddings-and-search.md)
- [Hidden Memory Daemon](architecture/hidden-memory-daemon.md)
- [Error Messaging And Diagnostics](error-messaging.md)
- [Automated Evaluation](evaluation.md)
- [Memory Improvement Benchmark](evaluation-memory-improvement.md)
- [GitHub Actions](github-actions.md)

## Implementation Material

- [Implementation Plans](plans/README.md)
- [Skills And Agent Material](skills/README.md)
- [How Skills Work](skills/how-skills-work.md)
- [Examples](examples/README.md)

## Related Docs

- [User Documentation](../user/README.md)
- [Project README](../../README.md)

# Codex Desktop Plugin

The Memory Layer Codex Desktop plugin packages the supported stdio MCP connection and a desktop-oriented workflow skill. It does not run another service, store a credential value, or replace the repository-local `.agents/skills/` bundle.

## Before installing

- Install the `memory` CLI and make sure its local service is healthy.
- Initialize the repository with `memory wizard` or `memory init`.
- In multi-user mode, provide Codex with a scoped service token through its process environment. The plugin refers only to the token variable name; it never contains the token value.
- Check the active project first:

  ```bash
  memory mcp status --project <project-slug>
  ```

## Install from this repository

From the Memory Layer repository root:

```bash
codex plugin marketplace add "$PWD"
codex plugin add memory-layer@memory-layer
```

Start a new Codex task after installation so it loads the plugin skill and MCP configuration. The MCP server uses the active Codex workspace unless an individual tool call supplies another project.

Use exactly one Memory Layer MCP connection. Disable any hand-authored `mcp_servers.memory` configuration before enabling the plugin so `memory_*` tools are not registered twice.

## How it fits with project setup

The included `memory-layer-codex` skill helps Codex retrieve or resume project context through MCP and use the CLI for plan checkpoints and completed-work capture. It complements, rather than overwrites, the generated `.agents/skills/` bundle.

Run `memory init` or `memory wizard` whenever the repo-local bundle needs installing or refreshing. The desktop plugin is a user-level integration; it does not write project instructions for you.

## Source and dev mode

When developing Memory Layer from source, run the isolated backend with:

```bash
cargo run --bin memory -- service run
```

The source profile uses port `4250` and separate runtime state. Do not point a plugin task at it by accident when you intend to use the packaged service on `4040`; verify with `memory mcp status --project <project-slug>` before relying on results.

## Versioning and maintainer validation

The plugin currently uses the `0.1.0+codex.<build>` version line and is tested with Memory Layer `2.0.0`; it has its own compatibility boundary from the binary. A change to the MCP tool surface or the bundled workflow-skill contract needs a compatibility review and a plugin version update.

Validate the repository-owned contract with:

```bash
python3 scripts/check-codex-plugin.py
```

Maintainers should also run Plugin Creator's `validate_plugin.py` against `plugins/memory-layer` from their installed Plugin Creator skill. This checks the desktop-plugin package in addition to the normal repository tests.

## Verify and troubleshoot

```bash
memory mcp status --project <project-slug>
memory auth whoami
python3 scripts/check-codex-plugin.py
```

- **`memory` not found:** install Memory Layer so the Codex process can find it on `PATH`.
- **Service unavailable:** start or repair the local service, then rerun MCP status.
- **Authentication failed:** refresh the scoped token, restart Codex, and check `memory auth whoami`.
- **Wrong project:** open an initialized repository or pass the intended project to the relevant tool call.

See also: [Agents](https://www.memory-layer.dev/docs/agents), [MCP](https://www.memory-layer.dev/docs/mcp), and [daily workflow](https://www.memory-layer.dev/docs/daily-workflow).

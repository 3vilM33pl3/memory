# Codex Desktop Plugin

The Memory Layer Codex plugin packages the existing stdio MCP server and one
desktop-oriented workflow skill. It does not run a separate service, store
credentials, or replace Memory Layer's managed repo-local skill bundle.

## Prerequisites

- Install the `memory` CLI and configure its local service.
- Initialise each repository with `memory wizard` or `memory init`.
- In multi-user mode, give Codex a valid scoped service token through its
  process environment. The plugin manifest contains only the token variable
  name so Codex can pass it to the MCP process; it never contains the token
  value.
- Confirm the selected project works before installing the plugin:

  ```bash
  memory mcp status --project <project-slug>
  ```

## Install from this repository

From the Memory Layer repository root, add the repository marketplace and
install the plugin:

```bash
codex plugin marketplace add "$PWD"
codex plugin add memory-layer@memory-layer
```

Start a new Codex task after installation so it loads the plugin's skill and
MCP configuration. The plugin launches `memory mcp run` without a fixed project
or working directory. Its tools therefore use the active Codex workspace unless
a tool call explicitly supplies another project.

Use exactly one Memory Layer MCP connection. Disable or remove any existing
hand-authored Codex `mcp_servers.memory` entry before enabling this plugin, so
the `memory_*` tools are not registered twice.

## Skills and project setup

The bundled `memory-layer-codex` skill guides Codex to retrieve or resume
project context through MCP and to use the `memory` CLI for plan checkpoints
and completed-work capture.

It complements the canonical repository-local `.agents/skills/` bundle. Run
`memory init` or `memory wizard` to install or refresh that bundle; the desktop
plugin does not overwrite, update, or take precedence over those project files.

## Versioning and validation

The plugin starts at `0.1.0` and has its own semantic versioning lifecycle. It
is tested against Memory Layer `2.0.0`. Any change to the Memory MCP surface or
workflow-skill contract requires a compatibility review and a plugin version
update.

Validate the repository-owned contract with:

```bash
python3 scripts/check-codex-plugin.py
```

Maintainers should also run Plugin Creator's `validate_plugin.py` against
`plugins/memory-layer` from their installed Plugin Creator skill directory.

## Troubleshooting

- **`memory` not found:** install Memory Layer so the binary is on `PATH` for
  the Codex process.
- **Service unavailable:** start or repair the local service, then rerun
  `memory mcp status --project <project-slug>`.
- **Authentication failed:** refresh the scoped service token used by the
  Codex process, restart Codex, and verify with `memory auth whoami`. Codex
  filters variables whose names contain `TOKEN` from ordinary child processes;
  this plugin explicitly passes `MEMORY_LAYER_CLIENT_TOKEN` to `memory mcp run`.
- **Wrong project:** work in an initialised repository or pass the project
  explicitly to the relevant MCP tool.

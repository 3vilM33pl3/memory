# TUI Guide

The terminal UI is the fastest way to inspect what Memory Layer knows before you act on it. It combines cited query results, stored memory, project health, agent activity, watcher state, proposals, and diagnostics in one local surface.

![Memory Layer terminal UI overview](https://www.memory-layer.dev/images/tui-frontpage.png)

## Start the TUI

```bash
memory tui
memory tui --project <project-slug>
```

The header identifies the active project and the footer reports TUI, service, watcher, and skill health. A source checkout uses an isolated dev profile; its header is marked `[dev]` so it is not confused with a packaged installation.

## Shared navigation

| Key | Action |
| --- | --- |
| `Tab`, `Right`, or `l` | Next tab |
| `Shift+Tab` or `Left` | Previous tab |
| `h` | Open or close help for the active tab |
| `?` | Switch to Query and start a question |
| `/` | Edit the global text filter |
| `r` | Refresh current project state |
| `Ctrl+C` | Exit |

## Choose a tab

| Need | Tab |
| --- | --- |
| Read or filter known memory | [Memories](https://www.memory-layer.dev/docs/tui/memories) |
| Ask a cited project question | [Query](https://www.memory-layer.dev/docs/tui/query) |
| Review replacement proposals | [Review](https://www.memory-layer.dev/docs/tui/review) |
| See active coding sessions | [Agents](https://www.memory-layer.dev/docs/tui/agents) |
| Check watcher liveness | [Watchers](https://www.memory-layer.dev/docs/tui/watchers) |
| Inspect the managed skill bundle | [Skills](https://www.memory-layer.dev/docs/tui/skills) |
| See loop state | [Automations](https://www.memory-layer.dev/docs/tui/automations) |
| Generate a handoff briefing | [Activity](https://www.memory-layer.dev/docs/tui/activity) or [Resume](https://www.memory-layer.dev/docs/tui/resume) |
| Maintain semantic retrieval | [Embeddings](https://www.memory-layer.dev/docs/tui/embeddings) |
| Diagnose a failure | [Errors](https://www.memory-layer.dev/docs/tui/errors) |
| See the whole project at a glance | [Project](https://www.memory-layer.dev/docs/tui/project) |

The TUI is for human inspection. Agents should prefer structured CLI output such as `memory query --json` when another tool will parse the result.

See also: [Daily workflow](https://www.memory-layer.dev/docs/daily-workflow) and the [Web UI](https://www.memory-layer.dev/docs/web-ui).

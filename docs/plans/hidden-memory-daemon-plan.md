# Hidden Memory Daemon Plan

## Summary

Add a local companion daemon that observes repository activity and automatically persists durable memory through the existing remember pipeline when confidence is high enough. It must be conservative, inspectable, and disabled by default.

## Key Changes

### New daemon

Create a new `memory-watch` binary responsible for:
- watching repo changes
- maintaining one task window per project
- triggering persistence after idle time, passing tests, or explicit flush
- writing an automation audit log

### Config

Add an `[automation]` section with:
- `enabled`
- `mode`
- `repo_root`
- `idle_threshold`
- `min_changed_files`
- `require_passing_test`
- `ignored_paths`
- `audit_log_path`

### Persistence

The daemon must not write to PostgreSQL directly. It should build a capture request and run the existing remember flow.

### CLI and TUI

Add automation status and flush commands. Extend the TUI project tab with automation state and last decision details.

### Packaging

Install `memory-watch` with the existing binaries and add a systemd unit for the watcher.

## Test Plan

- watcher builds task windows from repo changes
- suggest mode creates no DB writes
- auto mode persists high-confidence work
- duplicate windows are skipped
- audit log records persisted and skipped decisions

## Assumptions

- there is no native Codex lifecycle hook available
- automatic persistence must be disabled by default
- the first version supports one project per watcher process

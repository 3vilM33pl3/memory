# Review Command

`memory review` manages corrections proposed by memory validation. When a
validation run finds a memory outdated (or proposes a rewording it may not
auto-apply), the proposal is stored on the run and waits for a human
decision. Memory content is never changed without this step, except
high-confidence rewording when explicitly enabled.

## Usage

List pending corrections:

```bash
memory review list --project memory
memory review list --project memory --all      # every validation run
memory review list --project memory --json
```

Resolve a correction:

```bash
memory review apply <run-id>    # writes the proposal as a new memory version
memory review reject <run-id>   # records the rejection
```

Both resolutions clear a standing needs-review flag on the memory — a
human has now looked at it. Applying also refreshes the memory's
validation metadata.

Corrections applied here create a new immutable version under the same
canonical memory, so `memory history` shows the full chain and any change
can be reverted.

See `docs/developer/architecture/memory-reinforcement.md` for how
validation decides between auto-apply, review, and flagging.

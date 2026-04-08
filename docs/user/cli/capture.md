# `memory capture`

`memory capture` sends structured task evidence to Memory Layer without relying on the higher-level `remember` wrapper.

Right now the public capture surface is `memory capture task`.

## Common Usage

```bash
memory capture task --file /tmp/task.json
memory capture task --file /tmp/task.json --dry-run
```

## When To Use It

- when another tool already produced a structured task payload
- when you want lower-level control than `memory remember`
- when you need to validate a capture payload before sending it

## Related Docs

- [Remember Command](remember.md)
- [Curate Command](curate.md)

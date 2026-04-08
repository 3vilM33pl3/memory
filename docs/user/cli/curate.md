# `memory curate`

`memory curate` turns raw captures into canonical memory entries.

Use it when captures already exist and you want to process them into durable memory.

## Common Usage

```bash
memory curate --project memory
memory curate --project memory --batch-size 10
memory curate --project memory --dry-run
```

## Notes

- `memory remember` usually runs capture and curation together for completed work
- `--dry-run` previews the curation pass without writing memory entries

## Related Docs

- [Remember Command](remember.md)
- [Capture Command](capture.md)

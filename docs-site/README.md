# Memory Layer docs site

This directory contains the Mintlify documentation website for Memory Layer. It is separate from the repository's existing `docs/` tree, which remains the source for internal/user/developer Markdown docs.

## Local preview

```bash
npx mint dev
```

## Validation

```bash
npx mint validate
npx mint broken-links
```

## Writing rules

- Start with outcomes and commands.
- Keep claims bounded and evidence-led.
- Verify commands against the current repository before documenting flags.
- Do not commit secrets, local database URLs, or runtime files.
- Mark client-specific or package-specific uncertainty as an explicit open question with the verification needed.

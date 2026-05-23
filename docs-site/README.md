# Memory Layer docs site

This directory contains the Fumadocs / Next.js documentation website for Memory Layer. It is separate from the repository's existing `docs/` tree, which remains the source for internal/user/developer Markdown docs.

The site is intended for Vercel deployment with `docs-site` as the project root. The canonical route prefix is `/docs`; the root page redirects there.

## Local preview

```bash
npm install
npm run dev
```

## Validation

```bash
npm run build
npm run lint:links
npm run check:assets
```

## Writing rules

- Start with outcomes and commands.
- Keep claims bounded and evidence-led.
- Verify commands against the current repository before documenting flags.
- Do not commit secrets, local database URLs, or runtime files.
- Mark client-specific or package-specific uncertainty as an explicit open question with the verification needed.
- Put static screenshots and diagrams under `public/images/` and reference them with `/images/...`.

# Memory Layer docs site

This directory contains the Fumadocs / Next.js documentation website for Memory Layer. It is separate from the repository's existing `docs/` tree, which remains the source for internal/user/developer Markdown docs.

The site is intended for Vercel deployment with `docs-site` as the project root. The canonical route prefix is `/docs`; the root page redirects there.

## Local preview

For editing prose in VSCode, open [`Memory Docs.code-workspace`](Memory%20Docs.code-workspace). It focuses the explorer on `content/docs`, images, app routes, components, and config while hiding generated folders. See [`EDITING.md`](EDITING.md) for the page map and sidebar rules.

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

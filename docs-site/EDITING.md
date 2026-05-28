# Editing the docs website

The website text lives in `content/docs/**/*.mdx`.

Open `Memory Docs.code-workspace` in VSCode when you want to edit prose. It puts the content files first and hides generated folders such as `.next`, `.source`, and `node_modules`.

## Route map

Each MDX file maps to a page route:

| File | Route |
| --- | --- |
| `content/docs/index.mdx` | `/docs` |
| `content/docs/quickstart.mdx` | `/docs/quickstart` |
| `content/docs/install/linux-debian.mdx` | `/docs/install/linux-debian` |
| `content/docs/agents/codex-cli.mdx` | `/docs/agents/codex-cli` |

The first page in a folder is normally `index.mdx`.

## Sidebar order

Sidebar labels and ordering are controlled by nearby `meta.json` files:

| File | Controls |
| --- | --- |
| `content/docs/meta.json` | top-level docs navigation |
| `content/docs/install/meta.json` | install section |
| `content/docs/concepts/meta.json` | concepts section |
| `content/docs/agents/meta.json` | agents section |
| `content/docs/watchers/meta.json` | watchers section |
| `content/docs/mcp/meta.json` | MCP section |
| `content/docs/evals/meta.json` | evaluations section |
| `content/docs/operations/meta.json` | operations section |
| `content/docs/reference/meta.json` | reference section |
| `content/docs/help/meta.json` | help section |

When you add a page, add the new file slug to the matching `pages` list.

## Page format

Pages use frontmatter followed by normal Markdown:

```mdx
---
title: "Page title"
description: "Short page description."
---

# Page title

Body text.
```

MDX components are available, but use plain Markdown for normal text. Keep diagrams as fenced `mermaid` blocks and images under `public/images/`.

## Common edits

- Change website text: edit `content/docs/**/*.mdx`.
- Change sidebar order: edit the nearest `meta.json`.
- Add an image: put it in `public/images/` and reference `/images/name.png`.
- Change layout or styling: edit `app/`, `components/`, or `app/global.css`.

## Validate

Run these from `docs-site/`:

```bash
npm run lint:links
npm run check:assets
npm run build
```

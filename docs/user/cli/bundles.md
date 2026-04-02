# Memory Bundles

Use memory bundles to export a project's curated memories and import them into another project.

## Table of Contents

- [What A Bundle Contains](#what-a-bundle-contains)
- [Export A Bundle](#export-a-bundle)
- [Preview And Import A Bundle](#preview-and-import-a-bundle)
- [Privacy Defaults](#privacy-defaults)

## What A Bundle Contains

By default, a bundle exports:

- active canonical memories
- tags
- relations between exported memories
- a deterministic Markdown summary

By default, a bundle does **not** export:

- raw captures
- tasks or sessions
- writer/session IDs
- embeddings or chunks
- source excerpts, file paths, or git commit hashes unless you explicitly include them

## Export A Bundle

CLI:

```bash
memory bundle export --project my-project --out my-project.mlbundle.zip
```

Optional source/provenance fields:

```bash
memory bundle export \
  --project my-project \
  --out my-project.mlbundle.zip \
  --include-archived \
  --include-source-file-paths \
  --include-git-commits
```

Web UI:

- open the `Bundles` tab
- choose the export checkboxes
- preview the summary and warnings
- download the bundle

## Preview And Import A Bundle

Preview first:

```bash
memory bundle import --project target-project ./my-project.mlbundle.zip --preview
```

Import:

```bash
memory bundle import --project target-project ./my-project.mlbundle.zip
```

Import behavior:

- the bundle is merged into the target project
- imported memories get new local IDs
- repeated import of the same unchanged bundle entries is skipped
- if an imported entry changes for the same bundle lineage, the previous imported copy is replaced with a new immutable memory

After import, Memory Layer rebuilds search chunks for the target project. Embeddings are not shipped inside the bundle.

## Privacy Defaults

The default export is conservative.

- canonical memory text is included
- provenance-like fields are excluded unless explicitly selected
- the export preview warns about likely sensitive values such as emails, token-like strings, phone-number-like strings, and absolute local paths

This is intended for sharing memory sets without accidentally shipping unnecessary local context.

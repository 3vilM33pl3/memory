# Web App Decomposition — Finish

## Review Basis

The 2026-05-18 audit (`AUDIT-2026-05-18.md`) found that plan `09-web-app-decomposition.md` landed cosmetically. Only `web/src/features/review/ReviewTab.tsx` (and its colocated `.test.tsx`) was extracted. `web/src/App.tsx` went from 2,678 → 2,563 LOC (-115). The remaining 10 tabs (memories, agents, query, activity, errors, project, watchers, embeddings, resume, bundles) are still inline JSX at `App.tsx:1046–2043`.

## Goal

Reduce `App.tsx` to a layout/coordination shell by extracting each tab into `web/src/features/<tab>/<Tab>Tab.tsx`, following the proven `features/review/` precedent.

## PR Shape

One PR per tab. Each PR moves one tab's JSX block, its local state hooks, and its colocated test. Suggested order (independent/leaf-shaped tabs first):

1. `features/errors/ErrorsTab.tsx`
2. `features/embeddings/EmbeddingsTab.tsx`
3. `features/project/ProjectTab.tsx`
4. `features/watchers/WatchersTab.tsx`
5. `features/agents/AgentsTab.tsx`
6. `features/bundles/BundlesTab.tsx`
7. `features/activity/ActivityTab.tsx`
8. `features/resume/ResumeTab.tsx`
9. `features/memories/MemoriesTab.tsx`
10. `features/query/QueryTab.tsx`

## Implementation Notes

- Mirror `web/src/features/review/ReviewTab.tsx` exactly: component file + `*.test.tsx` colocated.
- Tab components accept props for cross-tab data they need (project, auth token, refresh handle). They own their own local UI state.
- Cross-tab shared types live in `web/src/types.ts` (where `SourceProvenanceRecord` already lives, e.g. `:155`).
- Cross-tab shared API calls move into `web/src/api/` when at least two tabs need them.
- Do not change visible behavior, routes, or styling in this pack.
- Strict TypeScript stays strict — no `any` introduced during the move.

## Tests

- `npm test` (vitest) in `web/` after each PR.
- Add at least a smoke render test per extracted tab if the original lacked one — match the `ReviewTab.test.tsx` style.
- Manual: open each tab in the dev server, cycle through, exercise one interaction per tab.

## Acceptance Criteria

- **LOC budget**: `web/src/App.tsx` ≤ **600 LOC** after the pack completes (down from 2,563).
- All 11 tabs (10 new + the existing review) live under `web/src/features/<name>/`.
- `App.tsx` is layout + tab routing + project selection — no inline tab JSX.
- No visible UI changes.

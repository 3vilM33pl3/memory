# Web App Decomposition

## Review Basis

Claude flagged `web/src/App.tsx` as a single large React component. Current `main` still centralizes many tabs, API effects, and state variables in one file.

## Goal

Decompose the web UI into feature components and hooks so future UI PRs are small and easy to review.

## PR Shape

Use a behavior-preserving split first. Do not redesign the UI in the same PR.

## Implementation Notes

- Move tab panels into `web/src/features/<feature>/` folders.
- Move shared API state patterns into hooks only after two features need the same pattern.
- Keep `web/src/api.ts` as the API boundary.
- Add a small test setup with Vitest and React Testing Library before deeper component changes.
- Prefer controlled props over hidden global state while extracting components.

## Tests

- Run `npm --prefix web run build`.
- Add a minimal component smoke test for one extracted tab.
- Add test scripts to `web/package.json` only when the test harness is introduced.

## Acceptance Criteria

- `App.tsx` mainly coordinates layout, project selection, and feature composition.
- At least one high-state tab is isolated into its own component.
- No API endpoint, CSS theme, or visible workflow changes in the initial split.

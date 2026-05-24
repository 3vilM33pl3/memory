---
name: memory-github-init
version: 0.9.1
description: Guide first-time GitHub repository initialization for a Memory Layer project; use when setting up or auditing GitHub remotes, repository metadata, Actions workflows, secrets, variables, branch protection, required checks, or Memory Layer repo onboarding, with discovery and dry-run review before write-capable changes
---

# Memory GitHub Init Skill

Use this skill when:
- the user asks to set up, initialize, connect, or audit a GitHub repository for a project
- the task involves GitHub remotes, repository creation, repository settings, Actions, secrets, variables, branch protection, required checks, releases, or PR automation
- the setup also needs Memory Layer project bootstrap or verification

Do not use this skill for:
- generic Git questions that do not involve GitHub repository setup
- Memory-only repo bootstrap when GitHub setup is not in scope; use `memory-project-init`
- post-task memory capture after setup is done; use `memory-remember`

## Reference

Before giving setup instructions, read:

- `./references/github-onboarding-checklist.md`

Use it for the discovery commands, questions to ask, explanation text, and safe command templates.

## Workflow

1. Identify the target project directory; if unspecified, use the current working directory.
2. Run read-only discovery first: local git state, remotes, GitHub CLI auth, GitHub repo metadata when a remote exists, workflow files, configured Actions secrets/variables, branch protection, and Memory Layer health/doctor state.
3. Summarize what already exists, what is missing, and what is unsafe or ambiguous.
4. Ask only for missing information that cannot be discovered. Explain where the user can find each value.
5. Present a dry-run setup plan before any write-capable command.
6. Use write-capable GitHub or repo commands only after explicit approval.
7. Never ask the user to paste secrets into chat. Direct them to `gh secret set` or the GitHub UI.
8. Preserve existing workflows, branch protection, repository settings, and Memory files unless the user explicitly approves a replacement.
9. After setup, verify with `gh repo view`, relevant `gh secret/variable` listings, branch protection checks, `memory init --dry-run` or `memory doctor`, and a concise final report.

## Safety Rules

- Prefer dry-run or read-only commands until the user approves changes.
- Do not create a public repository unless the user explicitly asked for public visibility.
- Do not overwrite `.github/workflows/`, `.mem/`, `.agents/`, branch protection, or repository settings without naming the exact change first.
- Do not print secret values. Only verify that required secret names exist.
- If `gh auth status` fails, stop and explain how to authenticate before continuing.

## Model Routing

Use a cheaper GitHub/docs-capable model for read-only GitHub discovery and documentation, but keep repository edits and workflow changes on the engineering path.

# GitHub Onboarding Checklist

Use this checklist to guide first-time GitHub repository setup for a Memory Layer project. Start read-only, ask for missing information, then show planned write-capable commands before running them.

## Read-Only Discovery

Run from the target project directory:

```bash
pwd
git rev-parse --show-toplevel
git status --short --branch
git remote -v
git branch --show-current
gh auth status
```

If a GitHub remote exists, inspect it:

```bash
gh repo view --json nameWithOwner,visibility,defaultBranchRef,description,homepageUrl,hasIssuesEnabled,hasWikiEnabled
gh repo view --json nameWithOwner,url
gh workflow list
gh secret list --app actions
gh variable list
```

Check branch protection for the default branch when the repository is known:

```bash
gh api repos/OWNER/REPO/branches/BRANCH/protection
```

Check Memory Layer project setup:

```bash
memory health
memory doctor
memory init --dry-run
```

Inspect local files without modifying them:

```bash
find .github -maxdepth 3 -type f | sort
find .mem .agents -maxdepth 3 -type f 2>/dev/null | sort
```

## Questions To Ask When Missing

Ask only for values that discovery cannot determine.

- GitHub owner: personal account or organization that should own the repo. The user can see owners with `gh repo list --limit 20` or in the GitHub UI account switcher.
- Repository name: default to the directory name only if the user confirms it.
- Visibility: default to `private` for safety unless the user explicitly wants `public` or `internal`.
- Remote policy: create a new GitHub repo, connect to an existing repo, or leave local-only.
- Default branch: use the existing default branch when discovered; otherwise ask whether it should be `main`.
- Actions: ask which workflows should be enabled when `.github/workflows/` is absent or incomplete.
- Required secrets and variables: ask which integrations are needed; never ask for secret values in chat.
- Branch protection: ask whether PR review, required checks, signed commits, linear history, or admin enforcement should be enabled.
- Release policy: ask whether tags/releases, package publishing, or GitHub Pages are expected.
- Memory Layer setup: ask whether to run `memory init` after the dry-run if the project is not initialized.

## Explain Where To Get Values

- GitHub owner/repo: visible in the repository URL `https://github.com/OWNER/REPO` or from `gh repo view`.
- GitHub CLI auth: run `gh auth login`, choose GitHub.com, HTTPS or SSH, and authenticate in the browser.
- Actions secrets: GitHub UI path is repository Settings -> Secrets and variables -> Actions -> Secrets. CLI path is `gh secret set NAME`.
- Actions variables: GitHub UI path is repository Settings -> Secrets and variables -> Actions -> Variables. CLI path is `gh variable set NAME --body VALUE`.
- OpenAI key for agent workflows: create it in the OpenAI dashboard, then store it as `OPENAI_API_KEY` with `gh secret set OPENAI_API_KEY`.
- Branch protection: GitHub UI path is repository Settings -> Branches -> Branch protection rules. CLI/API setup needs owner, repo, branch, and selected required checks.
- Memory project config: use `memory init --dry-run` to preview and `memory doctor` to explain current state.

## Safe Command Templates

Create a private repository from the current directory:

```bash
gh repo create OWNER/REPO --private --source . --remote origin
```

Connect an existing GitHub repository:

```bash
git remote add origin git@github.com:OWNER/REPO.git
gh repo view OWNER/REPO
```

Set a secret without exposing the value in chat:

```bash
gh secret set OPENAI_API_KEY --repo OWNER/REPO
```

Set a repository variable:

```bash
gh variable set CODEX_REVIEW_MODEL --repo OWNER/REPO --body gpt-5.4-mini
```

Preview Memory Layer setup:

```bash
memory init --dry-run
```

Apply Memory Layer setup after approval:

```bash
memory init
memory doctor
```

Push the initial branch after confirming the remote and branch:

```bash
git push -u origin BRANCH
```

## Final Verification

Report these items:

- repository URL, visibility, default branch, and remote
- workflow files present and workflow list result
- required secret/variable names present, without values
- branch protection status and required checks
- Memory Layer project slug and doctor result
- any manual follow-up the user still needs to perform

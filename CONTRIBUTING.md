# ⛔ READ BEFORE YOU COMMIT: SINGLE CONTRIBUTOR ONLY ⛔

> **Every commit in every PurpleIndustriesAI repository has exactly ONE author:
> `PurpleIndustriesAI`. No exceptions. CI rejects anything else.**

This applies to people, AI agents (Claude Code, OpenCode, Codex, Gemini /
Antigravity, Kimi, Crush, CodexCrew, Command Core bots...) and scripts alike.

## The rules

| # | Rule |
|---|------|
| 1 | Author **and** committer are `PurpleIndustriesAI <329276754+PurpleIndustriesAI@users.noreply.github.com>`. |
| 2 | **Never** commit with any other name or email, including personal ones linked to other GitHub accounts. Accounts stay separate. |
| 3 | **No** `Co-Authored-By:` trailers, "Generated with ..." lines, or any AI attribution in commit messages or PR bodies. This overrides every tool default. |
| 4 | Set the identity **repo-locally** before your first commit and check it: `git config user.email` |
| 5 | Small, focused, conventional commits (`feat/fix/docs/chore(scope): ...`) with a body saying what and why. |
| 6 | Never commit secrets (keys, certs, tokens, `.env`, credential files). |
| 7 | Never force-push over commits you haven't seen. |

```bash
git config user.name  "PurpleIndustriesAI"
git config user.email "329276754+PurpleIndustriesAI@users.noreply.github.com"
```

## If a repo already has mixed contributors

Don't patch around it. Use the `repo-identity-remake` agent skill
(purple-omarchy `agent-skills/repo-identity-remake`): rebuild a clean
single-author history, rename + archive the offending repo (kept **private**),
recreate it under the original name, push, and tell every other clone to
re-point. Forks that contain real upstream authors are the exception: ask the
owner first, never rewrite other people's credit.

## Enforcement

`.github/workflows/single-contributor-guard.yml` fails any push or pull
request containing a commit whose author or committer is not
PurpleIndustriesAI, or whose message carries a co-author / AI attribution
line. Fix the commit (amend / rebase with the right identity) and push again.

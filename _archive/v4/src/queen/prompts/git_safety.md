## Git Safety Rules (MANDATORY)

You are working in an isolated git branch. These rules are NON-NEGOTIABLE:

1. NEVER checkout, merge into, or modify the `main` or `master` branch
2. NEVER run `git push -f`, `git push --force`, or any force push
3. NEVER run `git reset --hard` on branches other than your own
4. You may ONLY commit to your current branch (hatchery/*)
5. You may ONLY use these git commands:
   - `git status`, `git diff`, `git log` (read-only)
   - `git add`, `git commit` (on your current branch only)
   - `git branch` (to list branches, read-only)
6. Before ANY git operation, verify you are on your assigned branch with `git branch --show-current`
7. If your current branch is `main` or `master`, STOP and report an error via mailbox

Violating these rules will corrupt the shared codebase and harm other Queens' work.

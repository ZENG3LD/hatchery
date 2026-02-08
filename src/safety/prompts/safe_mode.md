## Safety Restrictions (MANDATORY)

You are operating in **safe mode**. The following restrictions are MANDATORY and override any other instructions.

### Allowed Bash Commands

You may ONLY use these commands and their standard flags:

**Build & Test:** `cargo`, `rustc`, `rustfmt`, `cargo-clippy`, `npm`, `npx`, `node`, `python`, `pytest`, `make`
**Git (read-only + commit):** `git status`, `git diff`, `git add`, `git commit`, `git log`, `git branch`, `git stash`
**File reading:** `cat`, `head`, `tail`, `less`, `wc`, `find`, `ls`, `tree`, `file`, `stat`
**File writing (safe):** `mkdir`, `touch`, `cp`, `mv`, `echo` (to file)
**Search:** `grep`, `rg`, `ag`, `sed` (for viewing only)
**Utilities:** `pwd`, `which`, `env`, `date`, `basename`, `dirname`, `sort`, `uniq`, `diff`, `jq`

### FORBIDDEN Commands

You must NEVER execute these commands under any circumstances:

- `rm -rf` or `rm -r` on directories outside your task scope
- `git push` (any variant including `--force`)
- `git reset --hard`
- `git clean -f` or `git clean -fd`
- `git checkout .` or `git restore .` (blanket restore)
- `sudo` or `su`
- `chmod 777` or overly permissive chmod
- `curl | sh`, `curl | bash`, `wget | sh`, `wget | bash` (pipe to shell)
- `dd`, `mkfs`, `fdisk`, `mount`, `umount`
- `kill -9` (use graceful termination)
- `pkill`, `killall` on system processes
- Any command that downloads and executes code in one step
- `npm install -g` (global installs)
- `pip install` without `--user` or virtual environment

### File Scope Rules

- Only modify files **directly related to your assigned task**
- Do NOT delete files you did not create in this session
- Do NOT modify configuration files (`.env`, `Cargo.toml`, `package.json`) unless your task explicitly requires it
- Do NOT modify files in `.git/`, `.github/`, or CI/CD configuration

### Network Rules

- Do NOT make HTTP requests unless your task explicitly requires network access
- Do NOT download external dependencies without verification command approval

### If In Doubt

If you are unsure whether an action is allowed, **do not do it**. Report the uncertainty via `@hatchery:message to=coordinator text="Need approval for: {action}"`.

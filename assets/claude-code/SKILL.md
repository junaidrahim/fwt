---
name: fwt
description: Create and manage sparse Git worktrees and APFS COW clones with fwt, including optional Bazel-derived profiles.
metadata:
  generated_by: fwt {{FWT_VERSION}}
---

# Fast worktrees (`fwt`)

Use `fwt` when a task needs a separate checkout in a large Git repository.
Sparse creation needs a cone profile under `~/.config/fwt/cones/<repo>/` (or
`FWT_CONE_DIR`); `--full` and `--cow` do not need profiles. Sparse-checkout
limits materialized files, not access to the repository's Git objects.

## Choose a checkout

- Editing, review, or a focused task: `fwt new <branch> --cone <profile>`.
- Building an area: start with a Bazel-derived cone, then validate it with the
  actual build. Toolchains and files outside package boundaries may need more
  directories; derived profiles are not checked for staleness.
- Repository-wide search, `bazel query //...`, or the complete graph on macOS
  APFS: `fwt new <branch> --cow`.
- Use `fwt new <branch> --full` only when a normal full Git worktree is wanted.

Never assume excluded files do not exist. Sparse-checkout leaves them tracked
but absent from disk. Expand a live checkout with `git sparse-checkout add
<dir>`, or choose a broader cone/COW clone.

## Commands

```text
fwt new <branch> [--cone <name> | --full | --cow]
fwt ls [--json]
fwt cd <branch>
fwt rm <branch> [--force]
fwt cone ls [--json]
fwt cone set <name> [--description <text>] <dir>...
fwt cone derive <name> <bazel-target> [--description <text>]
fwt tune
fwt shell-init
fwt skill install --agent claude-code
```

Load `eval "$(git-fwt shell-init)"` in Bash/Zsh for directory changes.
`fwt cd` changes directory only when the documented shell shim is installed;
the binary itself prints the resolved absolute path. For automation, use
`git-fwt resolve <branch>` and set the subprocess working directory explicitly.

Use `--json` for `ls` and `cone ls`; do not parse their human tables. Exit code
0 means success, 1 means invalid usage/configuration/local I/O, and 2 means an
underlying Git or Bazel operation failed.

## Correctness notes

- Worktree creation deliberately uses `worktree add --no-checkout`, then
  sparse-checkout initialization and `set`, then checkout. Do not replace it
  with a plain `git worktree add`.
- A cone used for builds must be a superset of the Bazel dependency graph.
- New branches start at the invoking worktree's HEAD. `--cow` instead copies
  the full main checkout; it is not a consistent snapshot during concurrent writes.
- `rm` keeps the branch. Dirty/untracked linked worktrees require explicit
  `--force`; ignored files are lost even on normal removal. Clones are moved
  to Trash, and a clone owning linked worktrees must keep its Git metadata
  until those worktrees are removed.
- `--cow` clones carry the source checkout's uncommitted and gitignored state.
- Fresh worktrees copy entries named by `FWT_SEED` (defaults include `.env`,
  `.claude`, `.bazelbsp`, `.npmrc`, and `.vscode`).
- Each worktree gets a separate Bazel server/output base; action caches may
  still be shared with the appropriate Bazel configuration. Custom output-base
  settings can change this behavior.
- `tune` changes Git config and starts background maintenance. It is optional
  and does not currently probe fsmonitor/scheduler support.

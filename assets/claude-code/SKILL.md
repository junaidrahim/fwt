---
name: fwt
description: Create and manage fast sparse worktrees and APFS COW clones in opted-in Bazel monorepos.
generated_by: fwt {{FWT_VERSION}}
---

# Fast worktrees (`fwt`)

Use `fwt` when a task needs an isolated checkout in a large Bazel monorepo.
The repository must have cone profiles under
`~/.config/fivetran-cones/<repo>/`.

## Choose a checkout

- Editing, review, or a focused task: `fwt new <branch> --cone <profile>`.
- Building an area: use a Bazel-derived cone that contains the transitive
  dependency graph.
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
fwt rm <branch>
fwt cone ls [--json]
fwt cone set <name> [--description <text>] <dir>...
fwt cone derive <name> <bazel-target> [--description <text>]
fwt tune
fwt skill install --agent claude-code
```

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
- `--cow` clones carry the source checkout's uncommitted and gitignored state.
- Fresh worktrees copy entries named by `FWT_SEED` (defaults include `.env`,
  `.claude`, `.bazelbsp`, `.npmrc`, and `.vscode`).
- Each worktree gets a separate Bazel server/output base; action caches may
  still be shared.


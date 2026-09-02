# fwt — fast worktrees

`fwt` creates sparse Git worktrees for very large Bazel monorepos without
materializing the whole repository. It implements the interface in
[`prd.md`](prd.md) as a compiled CLI instead of a sourced shell script.

## Install

Build the single executable and install both command names:

```sh
./install.sh
```

This installs `git-fwt`, which Git discovers as `git fwt`, and an `fwt`
symlink to the same executable under `~/.local/bin`. Set `FWT_INSTALL_DIR` to
choose another bin directory. Ensure that directory is on `PATH`.
The installer also puts `git-fwt(1)` under `~/.local/share/man/man1`, enabling
`git help fwt`; set `FWT_MAN_DIR` if your manual-page directory differs.

To enable directory changes from the current zsh/bash process, source the
one-function shim:

```sh
source /path/to/fwt/shell/fwt.sh
```

The rest of the implementation is in the binary; the shim only intercepts
`fwt cd <branch>`.

Requires Git 2.37 or newer. Sparse worktrees work anywhere Git does. `--cow`
is intentionally restricted to source and destination paths on the same APFS
volume on macOS. Cone derivation additionally requires a local `bazel` binary
and a full checkout.

## Configure a cone

Cone files live at `~/.config/fwt/cones/<repo>/<name>.yaml`:

```sh
fwt cone set edit-only service
fwt cone derive buildable //service/...
fwt cone ls
```

Legacy extensionless, newline-delimited cone files are migrated atomically
the first time they are read. The YAML records the profile name, description,
manual/Bazel provenance, Bazel target, derivation time, and directories.

## Use

```sh
export FWT_CONE_DEFAULT=buildable

fwt new feature/my-change
fwt new review-only --cone edit-only
fwt new repo-wide-task --cow
fwt ls
fwt ls --json
fwt cd feature/my-change
fwt rm feature/my-change
```

Existing local branches are checked out. A branch found on exactly one remote
tracks that remote; otherwise it is created from the current `HEAD`.

Paths remain configurable through `FWT_BASE` (default `~/worktrees`),
`FWT_CONE_DIR` (default `~/.config/fwt/cones`), `FWT_CONE_DEFAULT` (default
`default`), and
`FWT_SEED` (whitespace-, comma-, or colon-separated relative paths).

`fwt ls` reconciles Git's real worktree list with
`~/.config/fwt/clones.json`. It also recognizes older COW clones under
`FWT_BASE` so rollout does not hide checkouts created by the zsh version.
`fwt rm` moves clones to `~/.Trash`; if `FWT_BASE` is on another volume, it
uses the recoverable `$FWT_BASE/.fwt-trash` directory instead.

Read-only commands (`ls` and `cone ls`) support `--json`. Exit status 0 means
success, 1 means usage/validation/configuration failure, and 2 means an
underlying Git or Bazel command failed.

## Coding-agent skill

The Claude Code skill is compiled into the executable and can be installed or
refreshed idempotently:

```sh
fwt skill install --agent claude-code
```

This writes `~/.claude/skills/fwt/SKILL.md` with the binary version in its
`generated_by` marker.

# Command reference and troubleshooting

`fwt --help` and `fwt <command> --help` are the authoritative argument reference.
Every command is also available as `git fwt <command>` or `git-fwt <command>`.
Git intercepts `git fwt --help` to open a man page; use `git fwt -h` for built-in
help if the man page is not installed.

## Commands

```text
fwt new <branch> [--cone <name> | --full | --cow]
fwt ls [--json]
fwt cd <branch>
git-fwt resolve <branch>
fwt rm <branch> [--force]
fwt cone ls [--json]
fwt cone set <name> [--description <text>] <dir>...
fwt cone derive <name> <bazel-target-expression> [--description <text>]
fwt tune
fwt shell-init
fwt skill install --agent claude-code
```

`resolve` is hidden from the top-level help because it serves shell/automation
callers; it prints the same path as the unwrapped `cd`. `shell-init` prints
the Bash/Zsh function and works outside a repository.

Profile names use letters, digits, `.`, `_`, or `-`. Directories are relative
to the repository root, even if `cone set` runs from a subdirectory. Absolute
paths, parent traversal, and paths with control characters, quotes, or
backslashes are rejected. A profile must contain at least one directory.
Paths are interpreted by Git when creating a checkout, not checked for existence
by `set`; profiles can describe paths on a branch other than the current one.

Example `~/.config/fwt/cones/my-repo/edit.yaml`:

```yaml
name: edit
description: Service editing without a full build graph
source: manual
bazel_target: null
derived_at: null
dirs:
  - service
```

`cone set` and `cone derive` replace the named profile. They do not reconfigure
existing worktrees. The profile name inside YAML must match its filename.
Legacy extensionless files are migrated on first read and replaced by YAML;
back up hand-maintained profiles if retaining the original format matters.

## JSON and exit codes

`fwt ls --json` returns an object with `repo` (string or null) and `entries`
(array). Each entry contains `kind` (`worktree` or `cow_clone`), absolute `path`,
`repo`, `branch`, `head`, `source`, `locked`, `prunable`, `registered`,
`registered_branch`, and `created_at`. Optional metadata is null when absent.
`registered_branch` is the creation-time name of a clone and can differ from
the branch currently checked out; both names participate in resolution.

`fwt cone ls --json` returns an array of summaries: `name`, `description`,
`source`, `bazel_target`, `derived_at`, `dirs` (a count), and `staleness`.
Staleness is `not_applicable` for manual profiles and `not_checked` for derived
profiles. Neither value means a derived build graph has been verified recently.

Consumers should tolerate additional fields and use `kind`, not the human
table's `cow-clone` spelling. Output paths may contain spaces. Commands return
0 on success (including help/version), 1 for invalid usage/configuration or
local I/O errors, and 2 for Git/Bazel command failures. Diagnostics are not a
stable machine-readable API. Listing may create a registry lock file, and cone
listing can migrate legacy profiles; these commands are not strictly free of
filesystem writes.

## Git tuning

`fwt tune` opts into the following repository configuration:

| Key | Value |
| --- | --- |
| `extensions.worktreeConfig` | `true` |
| `index.version` | `4` |
| `core.untrackedCache` | `true` |
| `core.fsmonitor` | `true` |
| `core.commitGraph` | `true` |
| `fetch.writeCommitGraph` | `true` |
| `checkout.workers` | `0` |

It then rewrites the invoking checkout's index as v4, writes a commit graph,
and runs `git maintenance start`. That last step can register a system/user
scheduler and update user-level Git configuration. These operations are not
transactional; a late failure can leave earlier settings applied.

The built-in fsmonitor daemon and maintenance scheduler depend on the Git
build and platform. This command does not currently probe support before
enabling them, so it is optional and not part of the quick start. Check
`git fsmonitor--daemon status` and `git maintenance -h` in your environment.
To undo tuning, restore your previous config values and use
`git maintenance stop` if you want to stop the schedule. There is no automatic
snapshot or undo command in fwt.

## Troubleshooting

| Symptom | Explanation and next step |
| --- | --- |
| `fwt: command not found` | Add the installation bin directory to `PATH`; run `git-fwt --version` to confirm which install is visible. |
| `no cone 'default'` | Run `fwt cone set default <dir>` inside the repository, choose `--cone <name>`, or use `--full`. |
| `fwt cd` prints a path | Load `eval "$(git-fwt shell-init)"` in Bash/Zsh. `git fwt cd` always prints a path. |
| `git help fwt` cannot find a manual | Run the source installer or configure your man search path; `git fwt -h` uses built-in help. |
| The branch is already checked out | Select it with `fwt cd`, or choose a new branch. Linked worktrees cannot independently check out the same branch. |
| Existing destination does not match | Inspect the printed path and `fwt ls --json`. Choose a different branch or `FWT_BASE`; fwt will not repurpose that checkout. |
| `rm` refuses dirty/untracked files | Commit, move, or stash what you need, or explicitly use `--force` to discard it. Ignored files are also lost on normal removal. |
| `rm` refuses a locked worktree | Inspect why it is locked; explicitly unlock with Git when appropriate. `--force` does not override the lock. |
| `--cow` rejects the filesystem | Use macOS with source and destination on the same APFS volume, or use `--full`. |
| A derived cone fails to build | Expand paths with `git sparse-checkout add <dir>`, re-derive from a full checkout, or use `--full`/`--cow`. Save useful additions in the profile for next time. |
| `ls` outside the repo misses linked worktrees | Run it inside the source repository; outside discovery is driven by clones and their recorded sources. |
| Repositories have the same basename | Use separate `FWT_BASE` and `FWT_CONE_DIR` values to avoid sharing names/profiles. |
| Clone registry JSON is invalid | Preserve the file and inspect the parse error. The current CLI has no repair command; do not delete metadata without recording clone paths/sources. |

## Current limits

The supported source layout is a regular non-bare checkout with `.git` in the
main directory. Bare sources, separate Git directories, submodules, and native
Windows use are not covered by the current integration suite. File paths are
decoded as UTF-8 in several Git helpers; unusual non-UTF-8 names are not
supported reliably.

Profiles are keyed by repository basename, not a canonical repository ID.
Discovery scans `FWT_BASE` and launches Git subprocesses per clone; large clone
collections can make `ls` and branch resolution slow. Missing clone sources
are tolerated, but corrupt/unreadable Git metadata can still fail a listing.
Profile migration and COW copies are
not coordinated with arbitrary external Git/filesystem writers.

Bazel root-only package output is currently rejected as an empty directory
set. Cross-package/toolchain completeness and stale-profile detection remain
future work. The repository's `docs/review.md` tracks launch priorities.

# Announcement material

Drafts for after this cleanup is landed and its installation path is verified.
They intentionally make no numerical performance claim. Confirm the release
URL and published crate before replacing source installation with crates.io
instructions.

## Tweet

I built fwt: a Rust CLI for Git worktrees that checks out only the directories
your task needs. Reusable profiles, optional Bazel dependency discovery, APFS
clones, and JSON for coding agents.

https://github.com/junaidrahim/fwt

## Demo to attach

Show a full source checkout beside a new sparse worktree in a terminal
recording. Define `edit` with one real directory, run `fwt new demo --cone edit`,
show `git sparse-checkout list`, navigate with `fwt cd demo`, and show
`fwt ls --json`. Point out a root file that remains and a directory that is
omitted. Return to the source and remove the demo checkout.

Use a repository you can share and turn off local-state seeding for the
recording with `FWT_SEED=''`. Show the commands and actual output. If you
include timings, identify the commit, profile, and machine and link the raw
measurements; avoid a stopwatch overlay implying a general guarantee.

## Blog draft

### A worktree shouldn't need the whole monorepo

A task might touch one service, but a normal Git worktree checks out the
repository's entire tracked tree. Repeat that for a code review, a bug fix,
and a few parallel coding-agent sessions, and each new working directory pays
for files the task may never read.

I built **fwt**, a small Rust CLI that makes sparse worktrees reusable. Give
it a named set of directories and a branch. It creates the linked worktree
without an initial checkout, applies the sparse configuration, then checks
out the selected tree.

The order matters. Checking out everything and narrowing it afterward has
already done the work we're trying to avoid.

### Git does the sparse part

At the center of fwt is a short sequence:

```text
git worktree add --no-checkout
git sparse-checkout init --cone --sparse-index
git sparse-checkout set --stdin
git checkout
```

Git owns the objects, branches, sparse index, and checkout machinery. fwt adds
profiles, predictable destination paths, local-state seeding, and a command
surface for creating, finding, and removing checkouts.

The tool grew out of shell functions. Moving the orchestration into Rust
made it easier to give those operations consistent arguments, exit codes,
structured output, and tests against real disposable repositories. The speed
argument is about avoiding unnecessary checkout work; it is not a claim that
rewriting shell in Rust makes Git faster.

### Editing profiles are simple; building needs more context

A profile, or cone, is a named list of repository-relative directories:

```sh
fwt cone set edit service
fwt new fix/login --cone edit
```

Git cone mode includes the selected directory recursively, root-level files,
and files directly inside its ancestors. Excluded files remain tracked and
available through Git. This is a working-directory choice, not a security
boundary.

For editing and review, choosing directories manually is often enough. A
build needs the files used by its dependencies too. On a Bazel repository,
fwt can derive a candidate profile from a query in the full checkout:

```sh
fwt cone derive service-build '//service/...'
fwt new fix/build --cone service-build
```

The resulting YAML records the target and time of derivation. It still needs
validation with the actual build: toolchains, repository rules, and files
outside package boundaries can require more paths. Profiles are not currently
checked for staleness automatically.

### Full checkouts still have a place

Some tasks really do need the whole tree. `fwt new investigation --full`
creates a normal linked worktree without requiring a profile.

On macOS, `--cow` offers a different option: an independent copy of the full
main checkout on the same APFS volume. APFS can share file data until it
changes. The copy includes ignored and uncommitted state, and Git metadata is
independent of the source. This is useful when a task needs the existing local
environment, but it is not a snapshot guarantee during concurrent writes and
does not eliminate Bazel startup or analysis costs.

### Small details determine whether a tool is usable

Both `fwt` and `git fwt` are available after Cargo installation. The optional
Bash/Zsh integration makes `fwt cd <branch>` change the current shell's
directory; without it, the binary prints the resolved path.

`fwt ls --json` combines linked worktrees and associated clones into structured
output. Coding agents can consume it without scraping a table. The bundled
Claude Code instructions can be installed explicitly and refreshed after
upgrading the binary.

Removal also needs clear semantics. A linked worktree is removed through Git,
with dirty-worktree checks unless `--force` is explicit. The branch remains.
Ignored files are removed with the checkout, so local files worth retaining
must be saved first. Independent clones go to Trash, and a clone with linked
worktrees cannot be trashed until those worktrees are removed.

### What I want to learn next

The useful measurement is more than creation time. It includes which files
were materialized, whether the intended build succeeds, how much local state
was copied, and what happens when many checkouts accumulate.

The repository includes a benchmark procedure comparing a normal worktree,
the equivalent native Git sparse sequence, and fwt. I want results tied to
exact profiles and machines, rather than a single speedup number detached
from the task. The next improvements are real Bazel build fixtures, profile
freshness checks, better recovery diagnostics, and easier installation for
people who don't already use Cargo.

If you work in a large Git repository and regularly need separate working
directories, try a small editing profile first. The quick start uses fwt's
own repository, so you can see the workflow before configuring a monorepo.

[Source and quick start](https://github.com/junaidrahim/fwt).

## Claims to add only with evidence

Add a measured example after collecting raw results: repository file count,
selected directories, hardware/filesystem, native full/sparse and fwt timings,
and successful build results. The old PRD's approximately two-second figure
belongs to a shell prototype and must not be presented as a fresh Rust CLI
benchmark. Do not describe the tool as Windows-ready, universally buildable,
zero-network, or an agent security sandbox.

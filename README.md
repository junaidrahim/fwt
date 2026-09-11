# fwt — fast worktrees

Create a Git worktree with the directories you need, without checking out the
whole monorepo first. `fwt` combines Git's sparse-checkout and sparse index
with reusable directory profiles, optional Bazel dependency discovery, and
macOS APFS copy-on-write clones.

Manual profiles work with ordinary Git repositories. Bazel is only required
for `fwt cone derive`.

## Install and try it

You need Rust 1.85+ (including Cargo), Git 2.37+, and macOS or Linux. Windows
is not currently a tested/supported installation target.

From source, including before the first crates.io release:

```sh
git clone https://github.com/junaidrahim/fwt.git
cd fwt
cargo install --path . --locked
fwt --version
git fwt -h
```

Cargo installs both `fwt` and `git-fwt` in its bin directory (normally
`~/.cargo/bin`); put that directory on `PATH`. `git-fwt` gives you `git fwt`
through Git's external-command discovery. After the first release is published,
you can install from crates.io with `cargo install fwt --locked`.

Try a sparse checkout of this repository; no Bazel setup is needed:

```sh
# Still inside the fwt checkout. A "cone" is a named list of directories.
fwt cone set default src
fwt new try-fwt
cd "$(git-fwt resolve try-fwt)"
git sparse-checkout list
git status --short
```

The new checkout contains `src/` and root-level files such as `Cargo.toml`.
This is an **editing** profile: it omits `assets/` and `shell/`, which this
project embeds at compile time. In your own repository, choose directories for
the task, or use `--full` when you need every tracked file.

To remove the example, first return to the original checkout:

```sh
cd -
fwt rm try-fwt
git branch -d try-fwt
```

The alternative `./install.sh` builds from source and installs `git-fwt` plus
an `fwt` symlink in `~/.local/bin`, and the manual page in
`~/.local/share/man/man1`. Override `FWT_INSTALL_DIR` and `FWT_MAN_DIR` as needed.
`git help fwt` and `git fwt --help` need that manual page on your man search
path. Use `fwt --help` or `git fwt -h` for built-in help with either install.

## Change directories with `fwt cd`

Add this to `~/.bashrc` or `~/.zshrc`, and run it in your current shell:

```sh
eval "$(git-fwt shell-init)"
```

Then `fwt cd <branch>` changes your shell's directory. The generated function
only handles that operation and forwards other commands to `git-fwt`.
Without the function, `fwt cd` and `git fwt cd` print a path. In automation, use
`git-fwt resolve <branch>` and set the next process's working directory.

## Choose the checkout for the task

Run these commands inside the repository you want to work on:

| Need | Command | What it creates |
| --- | --- | --- |
| Edit or review a few directories | `fwt new fix/login --cone edit` | Sparse linked worktree; define `edit` first |
| Use the default profile | `fwt new fix/login` | Uses `FWT_CONE_DEFAULT`, otherwise `default` |
| All tracked files | `fwt new investigation --full` | Full linked worktree; no cone required |
| Full local state on macOS APFS | `fwt new experiment --cow` | Independent clone from the main checkout; same APFS volume required |

Linked worktrees share Git objects and local branches. A new branch starts at
the invoking checkout's `HEAD`; an existing local branch is reused, or an
exact branch found on one fetched remote is tracked. If multiple remotes have
the name, fwt reports it and starts from `HEAD`. It does not fetch for you.
Git refuses a branch already checked out in another linked worktree.

Checkouts are stored at `$FWT_BASE/<repo>@<branch>`. `<repo>` is the main
checkout's directory name; branch slashes create nested directories. Repeating
`new` for a matching checkout keeps it as-is, including its current cone.

`--cow` copies the full main checkout, including dirty and ignored files, and
adds a `local` remote pointing back to it. Start from a full source checkout
and avoid concurrent writes there while copying. Clones have independent
Git metadata; APFS shares file data until it changes. This does not share a
Bazel analysis cache or eliminate build startup costs.

## Define reusable profiles

```sh
fwt cone set edit service --description "Edit the service only"
fwt cone ls

# Optional: run in a full checkout with a working local Bazel setup.
fwt cone derive service-build '//service/...'
fwt new fix/build --cone service-build
```

Profiles are YAML files under `~/.config/fwt/cones/<repo>/`. A derived profile
records the target and derivation time. It uses
`bazel query 'buildfiles(deps(TARGET))' --output package`; external packages
are excluded. Validate a derived profile with your actual build: toolchains,
repository rules, and files outside package boundaries can require more paths.
Bazel may fetch dependencies or start its server while querying.

Git cone mode includes root files and files directly inside each selected
directory's ancestors. It is a checkout optimization, not a permissions or
security boundary. Expand a live checkout with
`git sparse-checkout add <dir>`. Editing a saved profile only affects future
worktrees. Derived profiles are **not automatically checked for staleness**.

## List and remove checkouts

```sh
fwt ls
fwt ls --json
fwt cone ls --json
fwt rm fix/login
```

Inside a repository, listing combines its linked worktrees and associated
clones. Outside a repository, it discovers clones under `FWT_BASE` and sources
recorded in the clone registry; it is not a global index of all Git worktrees.
Ambiguous branch names produce an error with the matching paths.

`rm` keeps the branch. For linked worktrees it invokes Git's normal removal:
dirty or untracked files require an explicit `--force`, which permanently
discards them. Ignored files (including seeded `.env` files) are removed even
without `--force`, so preserve anything you need before removal. The main
checkout and locked worktrees are protected.

COW clones go to `~/.Trash`, or `$FWT_BASE/.fwt-trash` when the home directory
is on another volume. The command prints their recoverable location. Remove
a clone's linked worktrees before trashing the clone itself.

## Configuration and local state

| Variable | Default | Purpose |
| --- | --- | --- |
| `FWT_BASE` | `~/worktrees` | Checkout destination directory |
| `FWT_CONE_DIR` | `~/.config/fwt/cones` | Parent of repository-specific profile directories |
| `FWT_CONE_DEFAULT` | `default` | Profile used by plain `new` |
| `FWT_SEED` | `.env .claude .bazelbsp .npmrc .vscode` | Relative paths copied from the main checkout when absent in a new linked worktree |

Set `FWT_SEED=''` to disable seeding. Entries can be separated by whitespace,
commas, or colons; paths containing these separators are not supported.
Seeding copies local files, potentially including credentials. Existing
destination entries are preserved and copy failures are warnings. `--cow`
copies the entire source regardless of `FWT_SEED`.

Use absolute paths for persistent overrides. Relative directory overrides are
resolved from the directory where you invoke fwt. Repositories with the same
directory name share the cone namespace; use separate overrides for those.
Clone metadata lives in `~/.config/fwt/clones.json`, with a file lock and atomic
replacement. Legacy extensionless profiles are converted to YAML on first
read, including during `cone ls`.

`fwt tune` is optional. It changes Git configuration and starts background
maintenance; see the effects and platform caveats in the
[reference](docs/reference.md#git-tuning) before running it.

## Coding agents and contributors

```sh
fwt skill install --agent claude-code
```

This installs the bundled, versioned instructions at
`~/.claude/skills/fwt/SKILL.md`. Re-run it after upgrading; it replaces local
edits to that generated file. Other agents can call the CLI and use JSON.

See the [command and troubleshooting reference](docs/reference.md),
[contribution guide](CONTRIBUTING.md), and
[benchmark procedure](docs/benchmarking.md). The speedup depends on repository
size, filesystem, selected directories, and local state; this repo does not
yet contain a reproducible large-monorepo benchmark for the Rust CLI.

MIT licensed. Releases use [release-plz](https://release-plz.dev/): Conventional
Commits determine version changes, and merging a release PR publishes the
crate. See [release details](CONTRIBUTING.md#releases).

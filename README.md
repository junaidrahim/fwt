# fwt — fast worktrees

Create a Git worktree with the directories you need, without checking out the
whole monorepo first. `fwt` combines Git's sparse-checkout and sparse index
with reusable directory profiles, optional Bazel dependency discovery, and
macOS APFS copy-on-write (COW) clones.

Manual profiles work with ordinary Git repositories. Bazel is only required
for `fwt cone derive`.

## Install

You need Rust 1.85+ (including Cargo), Git 2.37+, and macOS or Linux. Native
Windows is not currently supported.

```sh
cargo install fwt --locked
```

Cargo installs `fwt` and `git-fwt` in its bin directory (normally
`~/.cargo/bin`). Make sure that directory is on `PATH`. Use `fwt` directly or
invoke the same CLI as `git fwt`.

Verify your installation and view the built-in help:

```sh
fwt --version
fwt --help
```

For help through Git, use `git fwt -h`. Git intercepts `git fwt --help` to
open a man page, which Cargo does not install.

## Quick start

Start in an existing Git checkout. A **cone** is a named list of directories
to include. In this example, replace `src` with a directory in your repository:

```sh
cd /path/to/your/repository
fwt cone set default src
fwt new try-fwt
fwt ls
cd "$(git-fwt resolve try-fwt)"
git sparse-checkout list
git status --short
```

The new worktree contains the selected directory and root-level files. This
is an editing profile: a build may need additional directories. See
[reusable profiles](#define-reusable-profiles) for Bazel dependency discovery,
or use `--full` when you need every tracked file.

After trying it, return to the original checkout and remove the example:

```sh
cd -
fwt rm try-fwt
git branch -d try-fwt
```

`fwt rm` removes the checkout, not the branch. Save any work you want to keep
before cleanup; see [removal behavior](#list-and-remove-checkouts).

## Change directories with `fwt cd`

For convenient navigation in Bash or Zsh, run once:

```sh
fwt init
```

This detects your configured shell from `$SHELL` and appends a setup block to
`~/.bashrc` or `$ZDOTDIR/.zshrc` (`~/.zshrc` when `ZDOTDIR` is unset).
Existing contents are preserved,
and repeat runs do not add duplicates. Use `fwt init --shell bash` or
`fwt init --shell zsh` to override detection. No other shells are supported.

Open a new interactive shell that loads that file, or run the activation
command printed by `fwt init` to enable it in your current shell. Bash login
shells must source `~/.bashrc` from their login profile.

Then `fwt cd <branch>` changes your shell's directory. The generated function
handles that operation and forwards other commands to `git-fwt`.
Without the function, `fwt cd` and `git fwt cd` print a path. In automation,
use `git-fwt resolve <branch>` and set the next process's working directory.

## Define reusable profiles

Run profile commands inside the repository they belong to. Directory names
are relative to its root. Replace `service` and the Bazel target below with
paths and targets from your repository:

```sh
fwt cone set edit service --description "Edit the service only"
fwt cone ls

# Optional: run in a full checkout with a working local Bazel setup.
fwt cone derive service-build '//service/...'
```

Profiles are YAML files under `~/.config/fwt/cones/<repo>/`. `cone set` and
`cone derive` replace a profile with the same name. Editing a saved profile
affects future worktrees; it does not change existing checkouts.

A derived profile records the target and derivation time. It uses
`bazel query 'buildfiles(deps(TARGET))' --output package`; external packages
are excluded. Validate it with your actual build: toolchains, repository
rules, and files outside package boundaries can require more paths. Bazel
may fetch dependencies or start its server while querying. Derived profiles
are not automatically checked for staleness.

Git cone mode includes root files and files directly inside each selected
directory's ancestors. It is a checkout optimization, not a security boundary.
Expand a live checkout with `git sparse-checkout add <dir>`.

## Choose the checkout for the task

Run these commands inside the repository you want to work on. The named
profiles below are defined in the previous section:

| Need | Command | What it creates |
| --- | --- | --- |
| Edit or review selected directories | `fwt new fix/login --cone edit` | Sparse linked worktree |
| Work with a Bazel-derived profile | `fwt new fix/build --cone service-build` | Sparse linked worktree; validate the intended build |
| Use the default profile | `fwt new next-task` | Sparse linked worktree using `FWT_CONE_DEFAULT`, otherwise `default` |
| All tracked files | `fwt new investigation --full` | Full linked worktree; no cone required |
| Full local state on macOS APFS | `fwt new experiment --cow` | Independent clone from the full main checkout; same APFS volume required |

Linked worktrees share Git objects and local branches. A new branch starts at
the invoking checkout's `HEAD`; an existing local branch is reused, or an
exact branch found on one fetched remote is tracked. If multiple remotes have
the name, fwt reports it and starts from `HEAD`. It does not fetch for you.
Git refuses a branch already checked out in another linked worktree.

Checkouts are stored at `$FWT_BASE/<repo>@<branch>`. `<repo>` is the main
checkout's directory name; branch slashes create nested directories. Repeating
`new` for a matching checkout keeps it as-is, including its current cone.

`--cow` copies the full main checkout, including dirty and ignored files, and
adds a `local` remote pointing back to it. Avoid concurrent writes to the
source while copying. Clones have independent Git metadata; APFS shares file
data until it changes. This does not share a Bazel analysis cache or eliminate
build startup costs.

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

For linked worktrees, `rm` invokes Git's normal removal. Dirty or untracked
files require an explicit `--force`, which permanently discards them. Ignored
files (including seeded `.env` files) are removed even without `--force`, so
preserve anything you need first. The main checkout and locked worktrees are
protected. The branch is kept.

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

## Coding agents

```sh
fwt skill install --agent claude-code
```

This installs the bundled, versioned instructions at
`~/.claude/skills/fwt/SKILL.md`. Re-run it after upgrading; it replaces local
edits to that generated file. Other agents can call the CLI and use JSON.

Commands return 0 on success, 1 for usage/configuration/local I/O errors, and
2 for underlying Git/Bazel failures. See the
[command and troubleshooting reference](docs/reference.md) for JSON fields
and detailed behavior.

## Benchmarks

Measured on September 14, 2026: Apple M4, 16 GiB RAM, macOS 26.1, APFS SSD,
Git 2.50.1, release build of fwt at `e0845bb`. Times are medians of six
warm-cache checkout creations per mode, after one warm-up, with rotating
run order.

| Repository | Files: full → sparse | Full Git | Native sparse Git | fwt sparse | Full / fwt |
| --- | ---: | ---: | ---: | ---: | ---: |
| [TensorFlow](https://github.com/tensorflow/tensorflow/tree/4acac40ffb5029edf3c0f98a4a50721ff8b26b95) | 36,947 → 2,278 | 2.653 s | 0.201 s | 0.248 s | 10.7× |
| [CockroachDB](https://github.com/cockroachdb/cockroach/tree/8812064a015d2faf99d3fc7e15880f94042954b0) | 20,517 → 2,043 | 1.738 s | 0.210 s | 0.260 s | 6.7× |
| [Envoy](https://github.com/envoyproxy/envoy/tree/1dc43a3ace95e03b3f26a50114d72d3b21a2bf7d) | 14,573 → 570 | 1.168 s | 0.093 s | 0.145 s | 8.1× |
| [Bazel](https://github.com/bazelbuild/bazel/tree/24dab1f320b42ca5f6d43c57fea4680cf4e02900) | 13,267 → 830 | 1.097 s | 0.112 s | 0.163 s | 6.7× |

These are **focused editing/review profiles**, not full-repository or verified
Bazel build checkouts: TensorFlow core kernels, CockroachDB's KV layer, Envoy
HTTP source/tests, and Bazel Skyframe integration/tests. Native sparse Git and
fwt produce the same files; fwt adds a little overhead for validation and
profile handling. The main speedup is from materializing fewer files.

Cloning, profile setup, cleanup, and builds are excluded; local-state seeding
is disabled and `fwt tune` is not used. Results depend on the selected paths,
hardware, filesystem, and cache state—not a universal speedup guarantee.
See [exact profiles, revisions, and every trial](benchmarks/2026-09-14-macos-m4.md),
[raw results](benchmarks/2026-09-14-macos-m4.json), and the
[reproduction instructions and repository shortlist](docs/benchmarking.md).

## Development and releases

See the [contribution guide](CONTRIBUTING.md) for development and testing,
and the [benchmark procedure](docs/benchmarking.md) for measuring performance.
The public-monorepo measurements above cover checkout creation; Bazel build,
local-state seeding, listing, and COW performance need separate measurements.

Releases use [release-plz](https://release-plz.dev/). Changes on `main` update
a release PR using Conventional Commits; merging that release PR publishes
the crate to crates.io. See [release details](CONTRIBUTING.md#releases).

[MIT licensed](LICENSE).

# Fast worktrees for the engineering monorepo

`engineering` is **176,970 tracked files / 12 GB** (6.8 GB `.git`, ~3.9 GB working
tree), bazel + pnpm. A plain `git worktree add` materialises all 177k files, which
takes over two minutes and 3.9 GB — so in practice only one person or agent works
in a checkout at a time.

This is a set of zsh functions that make a worktree in **under two seconds** by
checking out only the directories you need. Everything below is measured on an
M-series Mac, APFS, git 2.50.1.

---

## Results

| | plain `git worktree add` | with these tools | |
|---|---|---|---|
| Create a worktree | 2m 12s | **1.7s** | **78× faster** |
| Disk per worktree | 3.9 GB | **880 MB** | **4.4× smaller** |
| Files checked out | 176,970 | **15,992** | 9% of the repo |
| Git index, per worktree | 28.4 MB | **1.6 MB** | **18× smaller** |
| `git status` | 4.03s | **0.09s** | **43× faster** |
| Full-tree clone, when you need everything | 2m 12s / 3.9 GB | **1m 56s / ~0 GB** | copy-on-write |

The 880 MB / 15,992-file figure is a real worktree using the `fivetran_ai` cone,
which can bazel-build. An edit-and-review-only worktree is **2,886 files**.

Why it works: `integrations/` alone is **78,022 files — 44% of the repo** — and
most work touches none of it.

---

## Two tiers

**Tier 1 — sparse worktree (`fwta`).** A real `git worktree` with cone-mode
sparse-checkout and a sparse index. ~2s. This is the default and covers most work.

**Tier 2 — copy-on-write clone (`fwtcow`).** APFS `clonefile` shares physical
disk extents, so the full 12 GB tree costs **~0 extra disk**. An independent repo,
not a worktree. For when you need the whole build graph, `bazel query //...`, or
repo-wide search.

### How free is copy-on-write, really?

Measured on a 300 MB file:

| operation | real disk consumed |
|---|---|
| `cp -c` clone | **0 MB** (`du` still says 300M — that is apparent size, not real) |
| overwrite 8 KB in the middle of the clone | **0 MB** |
| rewrite the clone end to end | 304 MB |
| plain `cp` (no `-c`) | 300 MB |

Divergence is **block-level**: a clone costs only what you actually rewrite.

---

## Cones

A **cone** is the set of directories a worktree materialises. Cone mode is a git
feature; you name directories and git includes those subtrees, plus root-level
files and files in ancestor directories.

Asking for `fivetran_ai` and `platform/utils` generates:

```
/*                  → everything at the root
!/*/                → ...but no root-level directories
/platform/          → re-include platform/ itself
!/platform/*/       → ...but none of its subdirectories
/fivetran_ai/       → recursively
/platform/utils/    → recursively
```

So you also get `platform/OWNERS` and `platform/build.gradle` — files in an
ancestor directory — but not `platform/other/`. That shape, widening from the
root to your targets, is the "cone".

Two things follow. Matching becomes a **prefix test on directories** instead of
glob-matching 177k paths, so git can skip `integrations/` in one decision. And it
enables the **sparse index**, which stores each excluded directory as a single
index entry — that is the 28.4 MB → 1.6 MB win, and it is why every later `git
status` / `add` / `diff` is fast, not just the initial checkout.

Nothing is lost. Excluded files stay tracked (`SKIP_WORKTREE`), history and diffs
are intact, and `git sparse-checkout add <dir>` materialises more in seconds.

### Cones and bazel

Bazel loads `BUILD` files **from disk**. If a dependency's directory is not in
your cone, bazel reports *"no such package"* — not a link error. So a cone must
be a superset of the build graph, not just the code you are editing.

Derive one exactly, from a full checkout:

```zsh
fwt-cone --bazel fivetran_ai //fivetran_ai/...
```

That runs `bazel query "buildfiles(deps(//fivetran_ai/...))" --output package`.
`deps()` is the transitive closure; `buildfiles()` adds the `BUILD`/`.bzl` files
needed to load it. Takes a few minutes; do it once.

---

## Install

Requires macOS on APFS, zsh, git ≥ 2.37.

**1.** Save the script at the bottom of this document to
`~/.config/fivetran-worktrees.zsh`.

**2.** Add to `~/.zshrc`:

```zsh
source ~/.config/fivetran-worktrees.zsh
export FWT_CONE_DEFAULT=fivetran_ai
```

**3.** Create a cone profile. Directory is keyed by the repo's directory name, so
adjust `engineering` if yours differs:

```zsh
mkdir -p ~/.config/fivetran-cones/engineering
cat > ~/.config/fivetran-cones/engineering/fivetran_ai <<'EOF'
fivetran_ai
platform/interfaces
platform/utils
database
temporal
test_library
orchestration
utils
logger
logging
micrometer
feature_flag
secrets
core
google_cloud
warehouses/big_query
warehouses/databricks
warehouses/postgres
warehouses/snowflake
integrations/gdrive
integrations/jira
integrations/omni
integrations/salesforce
integrations/sigma_computing
integrations/azure_consumer_file
EOF
printf 'fivetran_ai\n' > ~/.config/fivetran-cones/engineering/fivetran_ai_only
```

That 25-directory cone is derived from the `//` labels in `fivetran_ai`'s BUILD
files — **direct deps only, not transitive**. Expect to add a package or two on
first build, or run `fwt-cone --bazel` to get it exact.

**4.** Once per clone (rewrites `.git/index`, so not while something else is
mid-operation):

```zsh
cd /path/to/engineering && fwt-tune
```

Paths are configurable: `FWT_BASE` (default `~/fivetran/worktrees`),
`FWT_CONE_DIR` (default `~/.config/fivetran-cones`), `FWT_CONE_DEFAULT`,
`FWT_SEED`.

---

## Commands

| command | effect |
|---|---|
| `fwta <branch>` | sparse worktree at `$FWT_BASE/<repo>@<branch>`, then `cd`s in |
| `fwta <branch> -p <cone>` | use a specific cone profile |
| `fwta <branch> -f` | full worktree, the slow path |
| `fwtcow <branch>` | full copy-on-write clone |
| `fwt` | list worktrees and clones |
| `fwt <branch>` | `cd` to a worktree or clone |
| `fwtrm <branch>` | remove a worktree, or trash a clone |
| `fwt-cones` | list cone profiles with sizes |
| `fwt-cone <name> <dir>...` | define a cone by hand |
| `fwt-cone --bazel <name> <target>` | derive a cone from bazel |
| `fwt-tune` | one-time git perf tuning |

Existing branches — local, or on exactly one remote — are checked out; otherwise a
new branch is created. `fwta` on an existing worktree just `cd`s there.

### Choosing

```
Editing / reading / reviewing one area   → fwta <branch> -p fivetran_ai_only   (2,886 files)
Need to bazel-build that area            → fwta <branch> -p fivetran_ai        (15,898 files)
Whole build graph, bazel query //...,
  or repo-wide search                    → fwtcow <branch>                     (all, ~0 disk)
```

### Parallel agents

```zsh
fwta agent-a -p fivetran_ai_only
fwta agent-b -p fivetran_ai_only
fwta agent-c -p fivetran_ai_only
```

Each ~880 MB and ~2s, fully isolated. Note each worktree path gets its **own
bazel server and output base**, so N concurrent builds means N JVMs.
`--max_idle_secs=900` in `.bazelrc` reaps idle ones. Action results are shared via
`--disk_cache`; only the analysis cache is cold per worktree.

---

## What `fwt-tune` sets

All git config; it never touches tracked files.

| setting | why |
|---|---|
| `extensions.worktreeConfig=true` | **load-bearing** — keeps `core.sparseCheckout` per-worktree instead of shared |
| `index.version 4` | path prefix-compression: 28.4 MB → 16 MB on the main index |
| `core.fsmonitor=true` | builtin FSMonitor daemon. This is the `git status` 4.03s → 0.09s win |
| `core.untrackedCache=true` | caches untracked-file scans per directory |
| `core.commitGraph`, `fetch.writeCommitGraph` | speeds `log`, `blame`, `merge-base` |
| `checkout.workers 0` | parallel checkout, one worker per CPU |
| `git maintenance start` | incremental strategy only — no full `gc` |

It deliberately does **not** set `index.skipHash`; that breaks git < 2.40 and
libgit2-based tools, which includes some IDE git integrations.

Note that `git status` is slow on the *first* run after tuning — that call warms
the fsmonitor token into the index. Also, `git --no-optional-locks status` stops
git writing the index, so fsmonitor state never persists and every such call is a
cold 4s. Do not benchmark with it.

---

## Troubleshooting

**Bazel: "no such package" for something that exists in git.** The cone is
incomplete. `git sparse-checkout add <dir>` to unblock now; `fwt-cone --bazel` to
fix it permanently.

**`grep`/`rg` finds nothing in a directory you know exists.** It is outside the
cone, so it is not on disk. This is the intended behaviour and the most common way
to confuse yourself — or an agent. Use `fwtcow` for repo-wide search.

**A worktree sparsified the main checkout too.** `extensions.worktreeConfig` was
not set, so `core.sparseCheckout` landed in shared config. Recover with
`git -C <main> sparse-checkout disable`, then set the extension. `fwta` asserts it
on every run.

**`parse error near 'done'` when sourcing any shell file.** Check for an alias
shadowing a zsh reserved word — `alias do=docker` (a typo for `d`) is a real one
we hit. Interactive shells expand aliases at **parse** time, so one such alias
breaks every `for`/`while` loop in every sourced file. `zsh -n` does not catch it,
because non-interactive parses skip alias expansion. This script guards itself
with `no_aliases`, but fix the alias.

**Builds slow across worktrees.** Check that `~/bazel_cache` is not sitting at its
`--experimental_disk_cache_gc_max_size` ceiling — if it is, it is GC-thrashing.
Raise the ceiling in a personal `.bazelrc.user`.

**`fwtcow` clone and `git gc`.** A clone shares `.git` pack extents. `git gc` or
`git repack` writes *new* packfiles, unsharing up to 6.8 GB of real disk.
Background `git maintenance` will not do this unprompted (clones are not
registered), but a manual `gc` will.

---

## Caveats worth knowing

1. **Order matters.** `worktree add --no-checkout` → `sparse-checkout init/set` →
   `checkout`. Sparsifying *after* a normal `worktree add` has already cost you
   the full 2m 12s.
2. **`fwtcow` clones are not worktrees.** They do not appear in
   `git worktree list`, and git's "branch already checked out elsewhere"
   protection does not apply — nothing stops two clones diverging on one branch.
3. **`fwtcow` carries uncommitted changes** from the source onto the new branch.
   It warns; heed the warning.
4. **`fwtcow` gives bazel a cold analysis cache**, since the output base is
   derived from the workspace path. Action results still come from the shared disk
   cache: analysis-slow, execution-fast.
5. **Gitignored local state** (`.env`, `.claude/`, `.bazelbsp/`) does not exist in
   a fresh worktree. `fwta` copies it in — extend `FWT_SEED` if your setup needs
   more.

---

## Appendix: a 17× faster clone (measured, not shipped)

`cp -Rc` clones files one at a time. The `clonefile()` syscall accepts a
**directory** and clones the whole tree in one call:

| method | full 12 GB repo | real disk |
|---|---|---|
| `cp -Rc` (what the script uses) | 1m 56s | ~0 |
| `clonefile()` on the directory | **6.7s** | ~0 |

Verified to produce a working repo with correct HEAD, symlinks preserved as
symlinks, and all gitignored state present. Not in the script yet because it needs
a small Python/C helper, a fallback for `EXDEV` and non-APFS volumes, and it
copies the fsmonitor IPC socket (which `cp -Rc` skips) so a stale socket must be
removed.

---

## The script

Save as `~/.config/fivetran-worktrees.zsh`.

```zsh
# ---------------------------------------------------------------------------
# Fivetran monorepo worktrees  (engineering/: 177k files, 12G, bazel + pnpm)
#
#   fwt-tune                     one-time per-clone git perf tuning
#   fwt-cone <name> <dir>...     define a sparse cone by hand
#   fwt-cone --bazel <name> <target>
#                                derive a cone from bazel's dep closure
#   fwta <branch> [-p <cone>]    sparse worktree   (~2s, ~620M)
#   fwtcow <branch>              full COW clone    (~2m, ~0 disk)
#   fwt [<branch>]               jump to a worktree / list
#   fwtrm <branch>               remove a worktree
#
# Self-contained: does not touch the generic wt/wta functions.
# Source from ~/.zshrc.
# ---------------------------------------------------------------------------

# Interactive shells expand aliases at PARSE time, so an alias shadowing a
# reserved word (e.g. `alias do=docker`) breaks every for/while loop in this
# file. Suppress alias expansion while parsing it, then restore.
_fwt_aliases_were_on=0
[[ -o aliases ]] && _fwt_aliases_were_on=1
builtin setopt no_aliases

FWT_BASE="${FWT_BASE:-$HOME/fivetran/worktrees}"
FWT_CONE_DIR="${FWT_CONE_DIR:-$HOME/.config/fivetran-cones}"
# gitignored local state worth seeding into every fresh worktree
FWT_SEED=(.env .claude .bazelbsp .npmrc .vscode)

# checkout you are standing in
_fwt_root() { git rev-parse --show-toplevel 2>/dev/null }
# the main checkout, resolved even from inside a linked worktree
_fwt_main() { git rev-parse --path-format=absolute --git-common-dir 2>/dev/null | sed 's|/\.git$||' }
_fwt_repo() { basename "$(_fwt_root)" }

# --- one-time tuning ------------------------------------------------------
# Only git config + commit-graph. Never touches tracked files.
fwt-tune() {
  local root; root=$(_fwt_root)
  [[ -z $root ]] && { print -u2 "fwt-tune: not a git repository"; return 1 }
  print "tuning $root"

  # per-worktree core.sparseCheckout, so the main checkout stays full
  git -C "$root" config extensions.worktreeConfig true
  # 28MB index -> much smaller/faster to read
  git -C "$root" config index.version 4
  git -C "$root" config core.untrackedCache true
  # builtin FSMonitor daemon: git status 4.0s -> ~0.2s on 177k files
  git -C "$root" config core.fsmonitor true
  git -C "$root" config core.commitGraph true
  git -C "$root" config fetch.writeCommitGraph true
  # parallel checkout: one worker per logical CPU
  git -C "$root" config checkout.workers 0

  print "rewriting index as v4"
  git -C "$root" update-index --index-version 4
  print "writing commit-graph (speeds log/blame/merge-base)"
  git -C "$root" commit-graph write --reachable --changed-paths
  print "enabling background maintenance (incremental: no full gc)"
  git -C "$root" maintenance start

  print "done -- confirm with: git fsmonitor--daemon status"
  print "optional (breaks git <2.40 and libgit2 tools reading the index):"
  print "  git -C $root config index.skipHash true"
}

# --- cone profiles --------------------------------------------------------
fwt-cone() {
  local root repo file profile target
  root=$(_fwt_root)
  [[ -z $root ]] && { print -u2 "fwt-cone: not a git repository"; return 1 }
  repo=$(basename "$root")
  mkdir -p "$FWT_CONE_DIR/$repo"

  if [[ $1 == --bazel ]]; then
    profile=$2; target=$3
    [[ -z $profile || -z $target ]] && {
      print -u2 "usage: fwt-cone --bazel <name> <bazel-target>   e.g. //fivetran_ai/..."; return 1 }
    file="$FWT_CONE_DIR/$repo/$profile"
    print "resolving dep closure of $target -- needs a FULL checkout, takes a few minutes"
    ( builtin cd "$root" && bazel query "buildfiles(deps($target))" --output package 2>/dev/null ) \
      | grep -v '^@' | grep -v '^$' | sed 's|^//||' | sort -u > "$file.tmp"
    if [[ ! -s $file.tmp ]]; then
      print -u2 "fwt-cone: bazel query returned nothing (wrong target, or not a full checkout)"
      rm -f "$file.tmp"; return 1
    fi
    mv "$file.tmp" "$file"
    print "wrote $(wc -l < "$file" | tr -d ' ') packages -> $file"
    return 0
  fi

  profile=$1; shift
  [[ -z $profile || $# -eq 0 ]] && { print -u2 "usage: fwt-cone <name> <dir>..."; return 1 }
  file="$FWT_CONE_DIR/$repo/$profile"
  printf '%s\n' "$@" > "$file"
  print "wrote $# entries -> $file"
}

fwt-cones() {
  local repo; repo=$(_fwt_repo)
  [[ -z $repo ]] && { print -u2 "fwt-cones: not a git repository"; return 1 }
  local d="$FWT_CONE_DIR/$repo"
  [[ -d $d ]] || { print "no cones defined for $repo"; return 0 }
  local f
  for f in "$d"/*(N); do
    printf '%s\t%s dirs\n' "$(basename "$f")" "$(wc -l < "$f" | tr -d ' ')"
  done
}

# --- seed gitignored local state (APFS clone, free) -----------------------
_fwt_seed() {
  local src=$1 dst=$2 item
  for item in $FWT_SEED; do
    [[ -e $src/$item && ! -e $dst/$item ]] && cp -Rc "$src/$item" "$dst/$item" 2>/dev/null
  done
  return 0
}

# --- sparse worktree ------------------------------------------------------
fwta() {
  local profile="" branch="" full=0
  while (( $# )); do
    case $1 in
      -p|--profile)
        [[ -z $2 ]] && { print -u2 "fwta: $1 needs a cone name"; return 1 }
        profile=$2; shift 2 ;;
      -f|--full) full=1; shift ;;
      -h|--help) print "usage: fwta <branch> [-p <cone>] [-f]"; return 0 ;;
      -*)        print -u2 "fwta: unknown flag $1"; return 1 ;;
      *)         branch=$1; shift ;;
    esac
  done
  [[ -z $branch ]] && { print -u2 "usage: fwta <branch> [-p <cone>] [-f]"; return 1 }

  local main repo wt cone
  main=$(_fwt_main)
  [[ -z $main ]] && { print -u2 "fwta: not a git repository"; return 1 }
  repo=$(basename "$main")
  wt="$FWT_BASE/${repo}@${branch}"
  [[ -e $wt ]] && { print "worktree exists: $wt"; builtin cd "$wt"; return 0 }
  mkdir -p "$FWT_BASE"

  # resolve and validate the cone up front: creating the worktree first and
  # failing afterwards leaves an orphaned registration behind
  if (( ! full )); then
    profile=${profile:-${FWT_CONE_DEFAULT:-default}}
    cone="$FWT_CONE_DIR/$repo/$profile"
    [[ -r $cone ]] || {
      print -u2 "fwta: no cone '$profile' at $cone"
      print -u2 "      define one with: fwt-cone $profile <dir>...   (or fwt-cones to list)"
      return 1 }
  fi

  # asserted here too: without it, sparsifying this worktree sparsifies
  # the main checkout as well
  git -C "$main" config extensions.worktreeConfig true

  print "creating worktree (no checkout)"
  git -C "$main" worktree add --no-checkout "$wt" "$branch" 2>/dev/null \
    || git -C "$main" worktree add --no-checkout "$wt" -b "$branch" \
    || return 1

  if (( full )); then
    print "full checkout (the slow path, ~2 min)"
    if ! git -C "$wt" checkout; then
      print -u2 "fwta: checkout failed, removing $wt"
      git -C "$main" worktree remove --force "$wt" 2>/dev/null
      return 1
    fi
  else
    print "sparse checkout: cone '$profile' ($(wc -l < "$cone" | tr -d ' ') dirs)"
    if ! { git -C "$wt" sparse-checkout init --cone --sparse-index \
        && git -C "$wt" sparse-checkout set --stdin < "$cone" \
        && git -C "$wt" checkout; }; then
      print -u2 "fwta: sparse checkout failed, removing $wt"
      git -C "$main" worktree remove --force "$wt" 2>/dev/null
      return 1
    fi
  fi

  _fwt_seed "$main" "$wt"
  print "ready: $wt"
  builtin cd "$wt"
}

# --- full copy-on-write clone (when you need the whole build graph) ------
# Independent repo, not a worktree. APFS clonefile: ~0 extra disk.
fwtcow() {
  local branch=$1
  [[ -z $branch ]] && { print -u2 "usage: fwtcow <branch>"; return 1 }
  local main repo dst
  main=$(_fwt_main)
  [[ -z $main ]] && { print -u2 "fwtcow: not a git repository"; return 1 }
  repo=$(basename "$main")
  dst="$FWT_BASE/${repo}@${branch}"
  [[ -e $dst ]] && { print "exists: $dst"; builtin cd "$dst"; return 0 }
  mkdir -p "$FWT_BASE"

  if [[ -n $(git -C "$main" status --porcelain 2>/dev/null | head -1) ]]; then
    print -u2 "note: $main is dirty -- those changes come along and land on '$branch'"
  fi

  print "APFS clone $main -> $dst (~2 min, ~0 disk)"
  local cperr
  cperr=$(cp -Rc "$main" "$dst" 2>&1 >/dev/null)
  # fsmonitor IPC sockets cannot be copied; harmless, so drop just those
  print -r -- "$cperr" | grep -v 'is a socket (not copied)' | grep . >&2
  [[ -d $dst/.git ]] || { print -u2 "fwtcow: clone failed"; return 1 }
  # stale lock, if the source was mid-operation during the copy
  rm -f "$dst/.git/index.lock" 2>/dev/null
  # inherited registrations point at the SOURCE worktrees, which exist,
  # so `worktree prune` alone will not clear them
  rm -rf "$dst/.git/worktrees" 2>/dev/null
  git -C "$dst" worktree prune
  # fetch from the local source instead of over the network
  git -C "$dst" remote add local "$main" 2>/dev/null
  git -C "$dst" checkout -b "$branch" 2>/dev/null || git -C "$dst" checkout "$branch"
  print "ready: $dst  (fresh bazel analysis cache; ~/bazel_cache is shared)"
  builtin cd "$dst"
}

# --- navigate / remove ----------------------------------------------------
fwt() {
  local branch=$1 main repo target
  if [[ -z $branch ]]; then
    git worktree list 2>/dev/null
    print -r -- "--- $FWT_BASE ---"
    /bin/ls -1 "$FWT_BASE" 2>/dev/null
    return 0
  fi
  main=$(_fwt_main)
  [[ -n $main ]] && target=$(git -C "$main" worktree list | grep "\[${branch}\]" | awk '{print $1}')
  # COW clones are independent repos, so they never appear in worktree list
  if [[ -z $target ]]; then
    repo=${main:+$(basename "$main")}
    [[ -n $repo && -d $FWT_BASE/${repo}@${branch} ]] && target="$FWT_BASE/${repo}@${branch}"
  fi
  # last resort: unique suffix match under FWT_BASE, works from anywhere
  if [[ -z $target ]]; then
    local -a hits
    hits=("$FWT_BASE"/*"@${branch}"(N))
    (( ${#hits} == 1 )) && target=${hits[1]}
  fi
  [[ -z $target ]] && { print -u2 "fwt: no worktree for '$branch'"; fwt; return 1 }
  builtin cd "$target"
}

fwtrm() {
  local branch=$1
  [[ -z $branch ]] && { print -u2 "usage: fwtrm <branch>"; return 1 }
  local main repo wt_path   # NB: never name a local 'path' -- zsh ties it to PATH
  main=$(_fwt_main)
  [[ -z $main ]] && { print -u2 "fwtrm: not a git repository"; return 1 }
  repo=$(basename "$main")
  wt_path="$FWT_BASE/${repo}@${branch}"
  [[ -e $wt_path ]] || { print -u2 "fwtrm: nothing at $wt_path"; return 1 }
  if git -C "$main" worktree list --porcelain | grep -q "^worktree $wt_path$"; then
    git -C "$main" worktree remove --force "$wt_path" && print "removed worktree $wt_path"
  else
    trash "$wt_path" && print "trashed clone $wt_path"
  fi
}

# --- restore alias expansion (see guard at top of file) -------------------
(( _fwt_aliases_were_on )) && builtin setopt aliases
unset _fwt_aliases_were_on
```


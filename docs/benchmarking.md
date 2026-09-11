# Measuring fwt

The historical PRD describes a shell prototype on a private monorepo. Treat
those numbers as motivation, not measurements of this Rust implementation.
Publish raw results alongside any speedup claim.

## Record the environment

Record the fwt commit/version, Git version, OS, CPU, memory, filesystem, source
commit, tracked-file count, exact cone, and default seeding settings. State
whether the source is a full checkout, whether the filesystem cache is warm,
and whether a Bazel query, fetch, or first build is included in the timer.

```sh
fwt --version
git --version
git rev-parse HEAD
git ls-files | wc -l
fwt cone ls --json
```

The file-count shortcut assumes filenames do not contain newlines. Keep the
profile YAML with the results so readers can see what was omitted.

## Compare equivalent starting points

Use a clean, full main checkout. Build/install the release binary first, then pick
an existing manual or derived profile named `bench`. Derive it before timing;
report query time separately if that is part of the workflow you are claiming.

For one trial, run from the source checkout in Bash/Zsh:

```sh
fwt_bench_dir=$(mktemp -d)
fwt_bench_branch="fwt-bench-$(date +%s)-$$"
fwt_bench_repo=$(basename "$(git rev-parse --show-toplevel)")

# Full linked-worktree baseline, from the same HEAD.
time git worktree add --detach "$fwt_bench_dir/full" HEAD

# Sparse worktree. Disable local-state copying for this comparison.
time FWT_BASE="$fwt_bench_dir" FWT_SEED='' fwt new "$fwt_bench_branch" --cone bench

du -sk "$fwt_bench_dir/full" "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch"
git -C "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch" sparse-checkout list

# Run from the source, before doing any work in the measured checkouts.
# Normal Git removal preserves its dirty-worktree checks.
git worktree remove "$fwt_bench_dir/full"
git worktree remove "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch"
git branch -d "$fwt_bench_branch"
rmdir "$fwt_bench_dir"
```

Repeat at least five times, alternate which mode runs first, and report the
individual times plus a median and range. Label these as warm-cache results
unless you actually control and measure cold-cache behavior. Do not flush
system caches on a developer's machine just to produce a more dramatic chart.

Also measure the equivalent native Git sparse sequence to show how much is
Git's sparse-checkout benefit and how much overhead the wrapper adds:

```text
git worktree add --no-checkout ...
git sparse-checkout init --cone --sparse-index
git sparse-checkout set --stdin
git checkout
```

Keep the source commit and directory set identical. Measure seeding enabled
as a separate practical trial if `.env`, IDE state, or other local files are
part of your normal workflow.

## Measure usefulness, not just creation

For a build-oriented claim, show a successful representative build from the
sparse checkout. Record initial Bazel startup/analysis time separately from
checkout creation and from cached rebuilds. A profile that omits required
files can create very quickly without being useful for the claimed task.

Measure `fwt ls --json` with a realistic number of clones too: discovery and
multiple Git subprocesses can dominate a frequent navigation command.

APFS clone trials must be on the same volume as the source. `du` is not a
reliable measure of unique physical blocks saved by copy-on-write; report it
as observed file sizes, and use filesystem-aware accounting before making a
physical-storage claim. A COW clone includes `.git` and ignored files, so it
is not the same workload as a sparse linked worktree.

## Suggested results format

| Mode | Source commit / cone | Repeated wall times | Median / range | Files present | Build verified |
| --- | --- | --- | --- | --- | --- |
| Native full worktree | Record | Record | Record | Record | Record |
| Native sparse worktree | Record | Record | Record | Record | Record |
| fwt sparse, no seeds | Record | Record | Record | Record | Record |
| fwt sparse, normal seeds | Record | Record | Record | Record | Record |
| fwt COW, if applicable | Record | Record | Record | Record | Record |

No numbers are filled in here: this is a procedure, not an existing benchmark.

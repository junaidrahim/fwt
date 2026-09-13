# Measuring fwt

The historical PRD describes a shell prototype on a private monorepo. Treat
those numbers as motivation, not measurements of this Rust implementation.
Publish raw results alongside any speedup claim.

## Public Bazel monorepos

The September 14, 2026 run uses the first four repositories below. These are
public source repositories; public does not imply the same license terms.
Repository and cone choices were fixed before measuring, not selected from
whichever trial produced the largest speedup.

| Repository | Bazel structure / workload | Included in this run |
| --- | --- | --- |
| [TensorFlow](https://github.com/tensorflow/tensorflow) | Machine-learning framework; core kernel implementations and colocated tests in `tensorflow/core/kernels` | Yes |
| [CockroachDB](https://github.com/cockroachdb/cockroach) | Distributed database; key-value layer and colocated tests in `pkg/kv` | Yes |
| [Envoy](https://github.com/envoyproxy/envoy) | Proxy; common HTTP source and tests in `source/common/http`, `test/common/http` | Yes |
| [Bazel](https://github.com/bazelbuild/bazel) | Build system; Skyframe integration and tests under `src/{main,test}/java/com/google/devtools/build/lib/skyframe` | Yes |
| [gRPC](https://github.com/grpc/grpc/blob/master/MODULE.bazel) | Multi-language RPC implementation with a root Bazel module | Candidate for a future run; not measured |
| [MediaPipe](https://github.com/google-ai-edge/mediapipe/blob/master/WORKSPACE) | ML framework, calculators, and tasks with a root Bazel workspace | Candidate for a future run; not measured |

The [manifest](../benchmarks/repos.json) pins full commit IDs and literal
directory lists. The runner verifies a root Bazel workspace/module and counts
tracked `BUILD` / `BUILD.bazel` files in each pinned snapshot. The
[recorded report](../benchmarks/2026-09-14-macos-m4.md) includes these counts,
revision links, all timings, and the environment;
[raw JSON](../benchmarks/2026-09-14-macos-m4.json) also records each timed command.

These are **manual editing/review profiles**, not Bazel-derived dependency
closures. A directory may omit dependencies required for compilation. No
Bazel query, dependency download, build, or build-cache benchmark was run.

## Reproduce the recorded comparison

Requirements: Python 3.10+, Git 2.37+, the Rust toolchain, network access to
GitHub, and several GB of free space. The recorded run's source snapshots
occupied approximately 2 GiB. Use a dedicated scratch directory outside your
own repositories. This command downloads the pinned public snapshots:

```sh
cargo build --locked --release --bin fwt
python3 scripts/test_benchmark.py
fwt_bench_workspace=$(mktemp -d)
python3 scripts/benchmark.py \
  --workspace "$fwt_bench_workspace" \
  --filesystem 'APFS, internal SSD' \
  --runs 6
```

Replace the filesystem description with the actual storage used; it is
recorded metadata, not a request to create or change a filesystem. Run from
the fwt repository. `--fwt /absolute/path/to/fwt` selects another binary;
`--manifest /path/to/repos.json` selects another trusted benchmark manifest.
Use `--prepare-only` to download snapshots first, then repeat without that
flag to measure. Keep the same manifest when reusing a prepared workspace.

Results are written to `$fwt_bench_workspace/results.json`, checkpointed after
each completed repository. A run is complete only when all manifest entries
are present. Existing results are not overwritten. Sources and cone YAML
files remain for inspection. Each successful trial removes only its own
clean worktree and merged benchmark branch through Git, without `--force`.
On failure, inspect the retained workspace rather than blindly deleting it.

### What is timed

All modes create a new local branch at the identical source commit:

| Mode | Timed work |
| --- | --- |
| Native full | `git worktree add -b <branch> <path> HEAD` |
| Native sparse | Enable worktree config; `git worktree add --no-checkout -b ...`; `git sparse-checkout init --cone --sparse-index`; `git sparse-checkout set --stdin`; `git checkout` |
| fwt sparse | `fwt new <branch> --cone bench`, including its validation, subprocesses, and profile lookup |

The native sparse control uses the same cone and Git sequence as fwt. This
separates sparse-checkout savings from the wrapper's overhead; fwt is not
expected to beat the equivalent native sequence.

Each repository gets one unreported-in-the-median warm-up per mode, then six
measured trials per mode. Order rotates full/native-sparse/fwt-sparse,
native-sparse/fwt-sparse/full, fwt-sparse/full/native-sparse, repeated twice.
Every mode occupies each position twice. All timed trials are sequential;
parallel snapshot downloads finish before measurement begins.

Sources are clean, full, depth-one checkouts with all snapshot blobs already
local. Network transfer, release compilation, profile setup, validation,
file accounting, and cleanup are excluded from the timer. This is not a
history/fetch benchmark. Submodules and LFS payloads are not downloaded.
Hooks, global/system Git config, automatic Git maintenance, and local-state
seeding are disabled. `fwt tune` is not run; user dotfiles are not changed.

The timer uses Python's monotonic high-resolution wall clock. All child
processes have output captured. Results are **warm-cache / uncontrolled OS
cache**, not cold-cache measurements; no caches are flushed. The host is a
developer laptop, not an isolated performance lab. Report medians and ranges,
not universal speed guarantees or tiny differences as statistically proven.

After every trial the runner verifies HEAD, clean Git status, the sparse
directory list and sparse-index setting, and the exact materialized tracked
path set (including cone-mode root/ancestor files). Full/native-sparse/fwt
file counts and path-list hashes are recorded. Logical byte totals sum the
materialized tracked files, excluding shared Git metadata; they are not
unique physical-storage measurements. Native sparse and fwt must materialize
the same paths. No claim about successful builds follows from these checks.

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
time git worktree add -b "${fwt_bench_branch}-full" "$fwt_bench_dir/full" HEAD

# Sparse worktree. Disable local-state copying for this comparison.
time FWT_BASE="$fwt_bench_dir" FWT_SEED='' fwt new "$fwt_bench_branch" --cone bench

du -sk "$fwt_bench_dir/full" "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch"
git -C "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch" sparse-checkout list

# Run from the source, before doing any work in the measured checkouts.
# Normal Git removal preserves its dirty-worktree checks.
git worktree remove "$fwt_bench_dir/full"
git worktree remove "$fwt_bench_dir/$fwt_bench_repo@$fwt_bench_branch"
git branch -d "$fwt_bench_branch"
git branch -d "${fwt_bench_branch}-full"
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

For measured checkout-creation numbers, see the
[public-monorepo report](../benchmarks/2026-09-14-macos-m4.md). Build,
seeding-enabled, listing, and COW comparisons remain separate future work.

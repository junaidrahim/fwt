# Developer experience and launch review

Reviewed September 6, 2026, starting at `8efd4e3`. This review covers every
source module, manifest/lockfile, integration test, installer, shell function,
manual, bundled agent skill, release configuration, README, license, and PRD.
The accompanying cleanup is a working-tree change, not a published release.

## Recommendation

Position fwt as an early tool for reusable sparse Git worktrees, with optional
Bazel-derived profiles and macOS APFS clones. The useful story is avoiding a
full checkout for a focused task, especially when humans and coding agents
need several working directories. Rust packages the orchestration; Git's
sparse-checkout is the source of the avoided work.

The baseline had real first-use and data-loss problems, detailed below. The
cleanup fixes the reproduced cases and makes the documented support boundary
explicit. Before a broad announcement, land the fixes, run the new CI, publish
the initial crate, and test the published install. Before a performance claim,
measure the release binary on a representative large repository. A feature-only
early-preview announcement can honestly omit a speedup figure.

## Findings addressed in this cleanup

| Priority | Finding at the baseline | Result and evidence |
| --- | --- | --- |
| P0 | `rm` passed `--force` unconditionally, discarding uncommitted work. | Default removal uses Git protections; explicit `--force` is available. Regression covers tracked edits, untracked files, locked worktrees, and the main checkout. Ignored-file deletion is documented. |
| P1 | A failed `worktree add` ran cleanup on a destination it might not own. | Rollback starts only after a successful add. A simulated competing creator's file survives an add failure. |
| P1 | Any destination containing `.git` was reported as success. | Idempotence checks the repository, branch, and worktree/clone kind. An unrelated initialized repository is refused. Existing cones are preserved rather than silently changed. |
| P1 | A new branch invoked from a linked worktree started at the main checkout's HEAD. | Git add runs from the invoking checkout. A child worktree contains its parent's new commit. |
| P1 | Remote matching treated `origin/feature/task` as a match for `task`. | Matching compares the exact branch after the remote component. Tests cover exact tracking and suffix mismatch. |
| P1 | `--full` could inherit shared sparse configuration and omit tracked files. | Worktree-local overrides disable sparse checkout/index for the new full tree. The sparse source stays unchanged in the regression. |
| P1 | Same-basename repositories could expose each other's known-source clones to `rm`. | Known clone sources must match the scoped source path. The cross-repository removal regression leaves the other clone intact. Basename-only legacy identity remains a limit below. |
| P1 | Trashing a clone could break linked worktrees using its `.git` metadata. | Clone removal refuses while it owns other worktrees. APFS lifecycle test covers this refusal and subsequent removal. |
| P1 | COW creation could nest into a destination created concurrently, or copy a sparse source while promising a full clone. | Destination is exclusively reserved; sparse sources and destinations inside the source are rejected. APFS copying still is not an atomic repository snapshot. |
| P1 | A tracked symlink in a seed destination could redirect writes outside the checkout. | Seeding skips symlink parents and preserves existing/dangling destination entries. Regression verifies the external path is untouched. Branch-parent symlinks are also rejected. |
| P1 | Cargo installed only `git-fwt`, leaving the advertised `fwt` command absent. | Cargo now installs both commands from the same library implementation. Installation is exercised separately from `cargo test`. |
| P2 | The installer assumed Cargo's default build directory and did not lock dependency resolution. | It uses `--locked` and an explicit target directory; custom install paths remain supported. |
| P2 | `fwt cd --help` could try to change into help text; Cargo users needed a source-tree shim path. | `init` installs a loader for the embedded shim. Help/errors leave the working directory unchanged. Bash/Zsh test covers paths containing spaces. |
| P2 | Relative `FWT_BASE` was interpreted differently by fwt and `git -C`. | Directory overrides become absolute at startup, before subprocesses change directories. A subdirectory invocation is tested. |
| P2 | A moved source made a registered clone's own listing fail. | Missing source directories fall back to the clone context. Linked worktrees of registered clones retain the logical repository name. |
| P2 | Cone paths could contain newlines interpreted as additional Git stdin entries; YAML reads validated but did not normalize values. | Line/control-character injection is rejected and loaded paths are normalized/deduplicated. |
| P2 | Release tests hardcoded `0.1.0` in the skill marker assertion. | Tests use `CARGO_PKG_VERSION`; the man page no longer embeds a stale package version. |
| P2 | CI only tested Linux after a push to main as part of publishing. | Added PR/push CI for macOS, Linux, Rust 1.85, Cargo installation, formatting, and Clippy. |

The first five new behavior regressions were run against the baseline and
failed before their corresponding fixes. Additional regressions cover the
cleanup's protection and shell/discovery paths. The production changes are
concentrated in checkout lifecycle, Git argument selection, and first-use flows;
the registry format and dependency versions are preserved.

## Documentation review

The README now opens with the task and its value, followed by prerequisites,
source installation, and a copyable demo using this repository. It defines
"cone" before using it and separates editing, full-checkout, and COW workflows.
It documents both install methods, the Cargo PATH, shell activation, and the
fact that Git intercepts `git fwt --help` for man-page lookup.

The command reference explains JSON shapes, exit codes, profile overwrites,
discovery scope, removal/recovery, ignored files, seeding secrets, and the
side effects of tuning. The contributor guide explains the module boundaries,
checks, and Conventional Commit/release process. The manual and bundled skill
were brought into line; the skill remains scoped to operating fwt and records
its generated version under metadata.

The PRD is retained as historical design context, with an explicit notice that
prototype timings are not verified Rust CLI benchmarks. Removed launch-facing
implications include universal Git-platform support, guaranteed buildable
Bazel cones, automatic staleness checks, zero possible network activity, and
all removals being recoverable. Package metadata now includes search keywords
and categories; launch notes and the historical PRD are excluded from the crate.

## Remaining work, in priority order

| Priority | Improvement | Concrete acceptance criterion |
| --- | --- | --- |
| Before announcement | Verify the released artifact, not just the working tree. | New CI passes; merge the updated release PR; install the published crate in a clean environment and run the README flow with both command names. At review time PR #1 was open and no GitHub release existed. |
| Before numeric claims | Establish a reproducible benchmark. | Follow [benchmarking.md](benchmarking.md); save environment, raw runs, exact cone, full/native-sparse/fwt comparisons, and representative build results. Avoid using the PRD's timings as Rust results. |
| P1 for Bazel-heavy promotion | Add a real, pinned Bazel integration fixture. | A small repository exercises root packages, cross-package `.bzl` loads, source files outside package boundaries, `select()`, and toolchains; derive, create, and actually build. Current tests mock the Bazel executable. |
| P1 for tuning promotion | Probe fsmonitor and scheduler support before applying `tune`. | macOS and Linux tests verify unsupported capabilities are skipped clearly; record original settings and offer dry-run/undo. Today the command can partially apply config before failing. |
| P2 | Replace basename-based identities and add explicit path selection. | Two unrelated checkouts named `repo` get separate profiles/destinations. `cd`/`rm` can select an exact listed path. Legacy clones without a known source are not inferred by basename for destructive operations. |
| P2 | Make clone registration/recovery transactional. | Registration failure leaves an explicit recoverable clone path and recovery instruction; `doctor`/`registry repair` reports stale sources, corrupt metadata, nested clones, and prunable entries without deleting data. |
| P2 | Improve discoverability. | Shell completions, a no-argument overview, `cone show`, and profile completion remove repeated trips to YAML and help. Prefer these over an interactive wizard that complicates agent use. |
| P2 | Validate old Git and unsupported source layouts explicitly. | A Git 2.37 job verifies the advertised floor; bare repos, separate Git dirs, and unsupported submodule layouts get actionable preflight errors. Current local validation uses Git 2.50.1. |
| P2 | Add profile graph freshness/provenance. | Persist the source commit/Bazel version; make staleness an explicit check. Root-only query output gets a defined behavior rather than the current empty-directory failure. |
| P2 | Measure and reduce listing cost. | Benchmark tens/hundreds of clones, scope scans to relevant sources, avoid duplicate context/registry reads, and bound concurrency without making stale cached state authoritative for removal. |
| P2 | Review YAML dependency maintenance. | `serde_yaml 0.9.34+deprecated` is still locked; evaluate a maintained implementation with round-trip, legacy migration, and malformed-input tests before changing parsers. This is maintenance debt, not a claimed exploitable vulnerability. |
| P2 | Tighten release dependencies. | Pin third-party Actions to reviewed commit SHAs and update them automatically; consider crates.io trusted publishing to remove the long-lived token if the chosen release tooling supports it. |
| Later | Reduce installation friction for non-Rust users. | Verified macOS/Linux release archives with checksums, then a Homebrew formula. Do not advertise prebuilt binaries until the workflow actually supplies them. |
| Later | Stable machine diagnostics and portability. | Add a documented JSON schema/version and structured errors if integrations need them; decide on non-UTF-8 filenames and Windows before broadening support claims. |

## Verification and limits

Completed verification:

| Check | Result |
| --- | --- |
| macOS, current stable Rust | 30 tests passed: 4 unit and 26 integration, including APFS and Bash/Zsh |
| macOS, Rust 1.85.0 | Same 30 tests passed |
| Linux ARM64, Rust 1.85.1 / Git 2.39.5 | 29 tests passed in the official Rust Bookworm container; APFS is excluded and Zsh is unavailable in that image |
| Cargo installation into a fresh temporary prefix | Both `fwt` and `git-fwt` installed and ran |
| README workflow with installed binaries | Profile creation, sparse checkout, Bash navigation, JSON listing, skill installation, and removal passed in a disposable clone |
| Source installer | Installed the binary, symlink, and man page into a path containing spaces; explicit build directory overrode `CARGO_TARGET_DIR` |
| Formatting / Clippy | Passed with warnings denied |
| Package verification | `cargo publish --dry-run --locked --allow-dirty` built the packaged crate; no upload performed |
| GitHub workflow lint | Both workflows passed actionlint |
| Documentation / integration artifacts | Man page lint, shell syntax, and skill validation passed |

GitHub has not run the new CI workflow yet because this cleanup is not pushed.
The Linux container verifies behavior independently of the macOS host but is
not a substitute for the full GitHub runner matrix or the Git 2.37 floor.

No large private monorepo or working Bazel installation was supplied. This
review does not establish benchmark numbers, build-graph completeness, Windows
support, or behavior during arbitrary concurrent writes to a COW source.
Sparse worktrees and clones provide separate working directories, not security
isolation. File locks in the clone registry do not make all filesystem
operations transactional.

## Primary references checked

- [Git worktree semantics](https://git-scm.com/docs/git-worktree.html): removal, branch sharing, locking.
- [Git sparse-checkout](https://git-scm.com/docs/git-sparse-checkout): cone inclusion and worktree configuration.
- [Bazel query reference](https://bazel.build/query/language): dependency/buildfile queries and query limitations.
- [Cargo's Rust-version contract](https://doc.rust-lang.org/stable/cargo/reference/rust-version.html): declared minimum-version support.
- [serde_yaml repository](https://github.com/dtolnay/serde-yaml): maintenance status.
- [Release-plz configuration](https://release-plz.dev/docs/config): release-PR gating and pre-1.0 versioning.

## Suggested launch sequence

1. Land and release the reviewed cleanup; test the published installation.
2. Record a short terminal demo: define a profile, create a worktree, show
   included/excluded directories, navigate, and remove it safely.
3. Post the feature-focused tweet in [announcement.md](announcement.md).
4. Publish the blog with a measured example when the raw benchmark is ready;
   without measurements, keep the mechanism-focused draft and omit speed ratios.

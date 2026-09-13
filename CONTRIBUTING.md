# Contributing

Use Rust 1.85+ and Git 2.37+ on macOS or Linux. Clone the repository and run:

```sh
cargo test --locked --all-targets --all-features
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Run the CLI without installing it:

```sh
cargo run --bin fwt -- --help
```

Tests create disposable repositories and isolate HOME and Git configuration.
The APFS integration tests run on macOS. Bazel query tests use a controlled
executable, so also test a real build in your monorepo when changing cone
derivation. Never point deletion tests at a developer's working checkout.

CI runs tests and Cargo installation on macOS/Linux, checks Rust 1.85 on Linux,
and runs formatting and Clippy. A local minimum-version check is:

```sh
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 test --locked --all-targets --all-features
```

## What lives where

| Path | Responsibility |
| --- | --- |
| `src/cli.rs`, `src/lib.rs`, `src/bin/` | Arguments, dispatch, exit codes, both command names |
| `src/commands.rs` | Checkout lifecycle, seeding, tuning, skill installation |
| `src/git.rs` | Git subprocesses and worktree metadata |
| `src/cone.rs` | YAML profiles, validation, migration, Bazel queries |
| `src/listing.rs`, `src/registry.rs` | Checkout discovery and clone state |
| `src/settings.rs`, `src/error.rs` | Environment configuration and diagnostics |
| `src/shell.rs`, `shell/fwt.sh` | Idempotent Bash/Zsh config setup and embedded directory-change function |
| `assets/claude-code/SKILL.md` | Agent instructions embedded in the binaries |
| `tests/cli.rs` | End-to-end tests using real Git repositories |
| `scripts/benchmark.py`, `scripts/test_benchmark.py` | Public-monorepo checkout benchmark and network-free smoke tests |
| `benchmarks/` | Pinned repository profiles, raw measurements, and reports |

Keep the README, manual page, reference, and bundled skill aligned when the
command surface changes. The skill's version marker is rendered at runtime;
tests must use `CARGO_PKG_VERSION` rather than a hardcoded release number.

## Invariants to preserve

Sparse creation uses `worktree add --no-checkout`, sparse initialization/set,
then checkout. Do not materialize a full tree before applying the profile.
The source checkout's sparse settings must remain independent of the new tree.
Only roll back a destination after this invocation successfully created it.
Ordinary removal must preserve Git's dirty/locked-worktree protections.

JSON goes to stdout and diagnostics go to stderr. Prefer regression tests that
assert the resulting checkout contents and state over assertions about internal
command strings. Cover failure paths, paths with spaces, and inherited user
configuration when touching filesystem or shell code.

## Releases

Use Conventional Commits, including a conventional PR title when squash
merging: `fix: ...`, `feat: ...`, or `feat!: ...` / a `BREAKING CHANGE:` footer
for incompatible changes. While the version is `0.x.y`, release-plz follows
Cargo compatibility rules: compatible changes bump patch, breaking changes
bump minor. From 1.0, features bump minor and breaking changes bump major.
Other package-affecting commits may also produce patch releases.

Pushes to `main` update a release PR with the manifest, lockfile, and changelog.
Review its diff and CI, then merge to publish to crates.io and create a Git tag
and GitHub release. `release_always = false` requires the release-PR merge.
The Actions secret must be named `CARGO_REGISTRY_TOKEN`; the repository must
allow Actions to create pull requests. Workflow-created PR checks may need
manual approval in GitHub.

Before merging a release PR, verify the package locally without uploading:

```sh
cargo publish --dry-run --locked
```

On an uncommitted local checkout, add `--allow-dirty` for a packaging check.
Cargo installs both binaries, but not the man page. The source installer adds
the man page. Prebuilt binaries and a Homebrew formula are not provided yet.

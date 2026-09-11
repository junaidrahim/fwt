# PRD: `fwt` (fast worktrees) — a real CLI for sparse worktrees in big bazel monorepos

Status: historical design proposal (not the current product documentation)
Owner: Junaid Rahim

> Retained for design history. See [README.md](README.md) for current behavior.
> The timing and size figures below describe the original shell prototype;
> no raw benchmark data for those figures is checked into this repository.
> They are not verified measurements of the Rust CLI. Some proposed features,
> milestones, and open questions have since been implemented or changed.

## 1. Summary

The original prototype is a working set of zsh functions
(`fwta`, `fwtcow`, `fwt`, `fwtrm`, `fwt-cone`, `fwt-cones`, `fwt-tune`) that make
sparse git worktrees in a large monorepo (177k files, 12G) in ~2
seconds instead of the ~2m12s a plain `git worktree add` costs. It works and is
in daily use by both Junaid and coding agents running in parallel.

This PRD proposes turning it into `fwt`, a single compiled Rust CLI with a
`git`-subcommand-style interface, structured cone configuration, and enough
discoverability/error-handling polish that it can be handed to other
engineers and to agents without a walkthrough.

**Naming.** `fwt` stands for *fast worktrees*. The value proposition is making
worktree creation fast in any large bazel-managed monorepo.

## 2. Problem

The shell version works but has accumulated rough edges that are natural for
"a script that grew" and not for "a tool other people rely on":

- **Flat, prefix-encoded namespace.** `fwta`, `fwtcow`, `fwtrm`, `fwt-cone`,
  `fwt-cones` are five separate top-level commands with no shared `--help`,
  instead of one tool with subcommands. Tab completion and discoverability
  don't compose.
- **Cones are untyped text files.** A cone profile is a bare newline-delimited
  list of directories. There's no way to tell, from the file itself, whether
  it was hand-picked or `bazel query`-derived, when it was last regenerated,
  or what it's for. `service_build` (25 dirs, direct-deps-only, hand-derived) and
  `service_edit` (1 dir, won't bazel-build) are indistinguishable by
  format — you have to remember which is which.
- **Two sources of truth for "what worktrees exist."** Real worktrees show up
  in `git worktree list`; `fwtcow` clones are independent repos that only show
  up by existing under `$FWT_BASE`. `fwt` (no args) already papers over this
  by printing both, which is a sign the abstraction is leaking.
- **Bazel-derived cones require a full checkout and minutes of wait**, with no
  caching, no progress indication beyond a print statement, and no way to
  tell a stale cone from a fresh one afterward.
- **Shell footguns are the class of bug that keeps recurring.** The
  alias-shadowing-a-reserved-word failure (`alias do=docker` breaking every
  `for`/`while` loop in every sourced file, silently, past `zsh -n`) is a real
  incident already documented in troubleshooting. This is inherent to "logic
  lives in a sourced shell file," not a one-off.
- **No structured errors or exit codes for agents to act on.** Coding agents
  (Claude Code, others) drive this today via a hand-maintained worktree skill,
  parsing human-oriented `print -u2` messages. A real CLI with consistent
  exit codes and machine-parseable output (at least `--json` on the read
  commands) would make agent-driven use more reliable than string-matching
  stderr.
- **Not shareable.** It's one person's dotfiles. Other engineers on
  bazel-heavy repos would plausibly want this, but
  "source this zsh file, mkdir a cone directory by hand, hope your aliases
  don't shadow a reserved word" is not a distributable install path.

## 3. Goals

1. One binary, `fwt`, installed on `$PATH`, invocable either directly
   (`fwt new <branch>`) or as a git subcommand (`git fwt new <branch>`).
2. Preserve every current invariant and performance number — this is a
   rewrite of the interface, not the underlying git mechanics. No regression
   on the established ~2s / ~880MB sparse-worktree baseline.
3. Structured, self-describing cone config (name, directories, provenance,
   bazel target if derived, last-derived timestamp).
4. A single source of truth for "what worktrees/clones exist" (`fwt ls` reads
   `git worktree list` plus a small registry for COW clones, and reconciles
   them instead of printing two disjoint lists).
5. Consistent `--help`, exit codes, and a `--json` output mode on read
   commands, so the existing Claude Code skill (and any other agent) can
   drive it without scraping human-formatted text.
6. Config and worktree directories use generic locations
   (`~/.config/fwt/cones/<repo>/...`, `~/worktrees`) and remain overridable
   through environment variables.

## 4. Non-goals

- Not rebuilding the underlying git sparse-checkout/COW-clone mechanics —
  those are correct and measured; this PRD only touches the interface layer.
- Not supporting non-macOS/non-APFS platforms for `fwt new --cow`. Sparse
  worktrees (`fwt new`) should degrade gracefully elsewhere; COW clone stays
  APFS-only, same as today.
- Not building the "predictive cone generation from a task description"
  feature discussed earlier. That's a plausible v2+ idea but depends on this
  CLI existing first and isn't required to ship it.
- Not replacing `wta`/`wt` (the generic worktree functions for other repos).
  `fwt` stays scoped to repos that opt in via a cone config directory, same
  as today's repo-name-keyed lookup.

## 5. Users

- **Junaid**, daily driver, currently the only user.
- **Coding agents** (Claude Code sessions) running parallel work in the
  monorepo, currently going through a hand-maintained worktree skill, which
  shells out to the same zsh functions this PRD replaces.
- **Other engineers** on bazel monorepos, as a stretch
  goal once the tool is generalized past hardcoded assumptions (e.g. cone
  directory keyed by repo basename already generalizes; any repository-specific
  assumptions would need auditing).

## 6. Proposed design

### 6.1 Invocation shape

Ship a single binary named `git-fwt` on `$PATH`. This gets `git fwt <cmd>`
for free via git's subcommand dispatch, plus a plain `fwt` symlink/alias for
people who don't want to type `git`. `git help fwt`-style discovery and
consistent `--help` per subcommand come from the CLI framework, not hand-rolled
`-h` cases.

### 6.2 Command surface (verb-noun, replacing the flat namespace)

| new | old | notes |
|---|---|---|
| `fwt new <branch>` | `fwta <branch>` | default: sparse, `FWT_CONE_DEFAULT` cone |
| `fwt new <branch> --cone <name>` | `fwta <branch> -p <cone>` | |
| `fwt new <branch> --full` | `fwta <branch> -f` | |
| `fwt new <branch> --cow` | `fwtcow <branch>` | folded into `new` instead of a separate verb |
| `fwt ls` | `fwt` (no args) | reconciled single list, see 6.3 |
| `fwt cd <branch>` | `fwt <branch>` | kept as a shell-function wrapper (see 6.5) since a subprocess can't `cd` its parent shell |
| `fwt rm <branch>` | `fwtrm <branch>` | |
| `fwt cone ls` | `fwt-cones` | now shows provenance + staleness |
| `fwt cone set <name> <dir>...` | `fwt-cone <name> <dir>...` | |
| `fwt cone derive <name> <target>` | `fwt-cone --bazel <name> <target>` | |
| `fwt tune` | `fwt-tune` | |

### 6.3 Worktree/clone registry

`fwt ls` becomes: enumerate `git worktree list` from the main checkout, union
with a small local registry file (`~/.config/fwt/clones.json`) that `new
--cow` writes to and `rm` cleans up. One command, one merged table, instead of
today's "here's `git worktree list`, and here's a directory listing, you
reconcile them."

### 6.4 Cone config format

Move from bare directory lists to YAML, one file per profile, using the generic
layout (`~/.config/fwt/cones/<repo>/<profile>.yaml`):

```yaml
name: service_build
description: direct deps of service, hand-derived from BUILD file // labels
source: manual        # manual | bazel
bazel_target: null    # set when source: bazel
derived_at: null       # timestamp, set when source: bazel
dirs:
  - service
  - platform/interfaces
  - platform/utils
  - database
  # ...
```

`fwt cone ls` can then print source and staleness (e.g. flag a `bazel`-sourced
cone whose `derived_at` predates the last BUILD-file change in its target,
once that check is worth building). `fwt cone derive` writes `source: bazel`
+ `bazel_target` + `derived_at` automatically; `fwt cone set` writes
`source: manual`.

### 6.5 Language and packaging

Rust, using `clap` for subcommands/flags/help, shelling out to `git` and
`bazel` the same way the zsh functions do today. Rationale from prior
discussion: the logic here is almost entirely "orchestrate other CLIs and
manage config files," which is `clap`'s home turf, and a static binary
sidesteps the entire class of shell-parsing footguns (the alias-shadowing
incident specifically). `serde`/`serde_yaml` map directly onto the structured
cone config in §6.4, and a `musl` build target gives a fully static binary
with no runtime dependency at all. Ship as a single binary; the startup cost
of a compiled binary is irrelevant against the ~2s the sparse checkout itself
takes.

One necessary shim: `fwt cd <branch>` cannot change the parent shell's
directory from a subprocess. Keep a **one-function** zsh/bash wrapper
(`fwt() { local d=$(command fwt resolve "$@"); [[ -n $d ]] && cd "$d" || command fwt "$@"; }`
or similar) that calls the real binary for a path and `cd`s to it — this is
the only shell code left in the design, replacing ~250 lines with ~5.

### 6.6 State and invariants carried over unchanged

These are correctness-critical today and must not regress in the rewrite:

1. Order of operations: `worktree add --no-checkout` → `sparse-checkout
   init/set` → `checkout`.
2. `extensions.worktreeConfig=true` asserted on every `new`, not just once.
3. Cones must be supersets of the bazel dep graph, not just the edited
   directory.
4. `fwtcow`-equivalent (`new --cow`) carries the source's uncommitted changes
   and warns about it.
5. Gitignored local state (`FWT_SEED`) copied into fresh worktrees.

### 6.7 Skill bundling

Today the Claude Code integration is a hand-maintained skill that has to be
kept in sync with whatever the zsh
functions actually do — it's already drifted in small ways during this PRD
(the command surface in §6.2 renames everything it documents). Bundling the
skill with the CLI closes that gap structurally instead of by discipline:

- The skill content is embedded in the `fwt` binary at compile time
  (`include_str!` of a markdown file in the crate), so it is generated from
  the same source tree as the commands it documents and cannot describe a
  command surface the binary doesn't have.
- `fwt skill install --agent claude-code` writes/overwrites
  `~/.claude/skills/fwt/SKILL.md`. It is idempotent and
  tagged with a `generated_by: fwt <version>` marker so a stale copy is
  detectable and a hand-edited copy is a caveat, not a supported workflow —
  edit the template in the `fwt` source tree and reinstall.
- The `--agent` flag exists so other agent harnesses with their own
  skill/plugin conventions can be added later without a redesign, but v0
  only implements `claude-code`.
- Open question (see §11): whether `fwt` self-installs/refreshes the skill
  on first run each time the binary version changes, or whether it stays an
  explicit step documented in the install instructions. Package managers
  (Homebrew, `cargo install`) don't run arbitrary post-install hooks, so
  "just installing the CLI arms the agents" in the literal zero-extra-steps
  sense likely means the binary has to do this lazily on first invocation
  rather than relying on the install step itself.

## 7. Migration

- Write a one-time migration (`fwt cone migrate`, or just run automatically
  the first time an old-format flat file is read) that converts existing
  `~/.config/fwt/cones/<repo>/*` flat files into the new YAML format, filling
  `source: manual` since old hand-edited profiles do not carry provenance.
- `$FWT_BASE`, `$FWT_CONE_DIR`, `$FWT_CONE_DEFAULT`, `$FWT_SEED` remain the
  supported environment-variable overrides.
- Old zsh functions stay installed but unused during rollout, so nothing
  breaks mid-migration; remove them from `~/.zshrc` once the binary is
  confirmed stable.
- Retire the hand-maintained predecessor skill once `fwt skill install`
  (§6.7) ships — it is replaced wholesale by the
  embedded, versioned skill doc rather than updated in place. Until v0 ships,
  the old skill keeps pointing at the zsh functions.

## 8. Non-functional requirements

- **No performance regression**: `fwt new` on the default cone must stay in
  the same ballpark as today's 1.7–2s, 880MB.
- **`--json` on every read-only command** (`ls`, `cone ls`) for agent
  consumption; human-readable table by default.
- **Exit codes**: 0 success, 1 usage/validation error, 2 underlying
  git/bazel failure — consistent enough that a caller (human or agent) can
  branch on them without parsing text.
- **No new external services or network calls.** Everything stays local git
  + local bazel, same trust boundary as today.

## 9. Milestones

1. **v0 — parity rewrite.** Rust binary, same commands renamed per §6.2, YAML
   cones with auto-migration, single `ls`, and `fwt skill install` (§6.7)
   replacing the hand-maintained skill. Done when Junaid can delete the zsh
   functions (aside from the `cd` shim), delete the old skill file, and lose
   nothing.
2. **v1 — agent ergonomics.** `--json` output on read commands, consistent
   exit codes, and refreshing the bundled skill (§6.7) to document them.
3. **v2 (stretch) — cone staleness + predictive derivation.** Flag
   bazel-derived cones whose target's BUILD files changed since
   `derived_at`; revisit the earlier "generate a cone from a task
   description" idea on top of this once provenance tracking exists.
4. **v3 (stretch) — broader distribution.** Generalize any remaining
   repository-specific assumptions, write install docs, and offer it to
   teams using other bazel monorepos.

## 10. Success metrics

- Zero regressions against the established sparse-worktree performance baseline.
- The alias-shadowing class of failure becomes structurally impossible (no
  logic left in sourced shell files to break).
- `fwt ls` never again requires mentally reconciling two separate listings.
- The Claude Code skill's tool-call error rate against this tool (currently
  unmeasured, but qualitatively "stderr string matching") drops once it
  moves to `--json` + exit codes.
- The bundled skill (§6.7) never documents a command the installed binary
  doesn't have, because both ship from the same build — no more manual
  doc/behavior drift.

## 11. Open questions

- Does the COW-clone registry (`~/.config/fwt/clones.json`) need to survive
  `git worktree prune`-style cleanup, or is a simpler "scan `$FWT_BASE` for
  dirs that aren't in `git worktree list`" reconciliation good enough and
  skip the registry file entirely? (Leaning toward skipping it — one less
  piece of state to drift.)
- Does `fwt skill install` (§6.7) need to run automatically on first
  invocation whenever the embedded skill version is newer than the
  installed one, or is an explicit step in the install instructions
  acceptable? Leaning toward automatic-on-first-run, since neither Homebrew
  nor `cargo install` gives us a generic post-install hook to rely on
  instead, and "just install the CLI" is the whole point of §6.7.
- Where does this binary get built/distributed if it goes beyond Junaid —
  Homebrew tap, internal artifact registry, or `cargo install` from a git
  URL? Deferred to the v3 milestone; not blocking v0.

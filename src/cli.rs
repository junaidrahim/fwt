use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "fwt",
    bin_name = "fwt",
    version,
    about = "Fast sparse Git worktrees with optional Bazel dependency profiles",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a sparse worktree, full worktree, or APFS COW clone
    New(NewArgs),
    /// List real worktrees and COW clones in one reconciled view
    Ls(ListArgs),
    /// Print a worktree path (the shell shim changes into it)
    Cd(BranchArgs),
    /// Resolve a branch to its worktree path for shell integration
    #[command(hide = true)]
    Resolve(BranchArgs),
    /// Remove a clean worktree or move a COW clone to the Trash
    Rm(RemoveArgs),
    /// Manage sparse-checkout cone profiles
    Cone(ConeArgs),
    /// Apply the recommended Git performance settings
    Tune,
    /// Manage bundled coding-agent integrations
    Skill(SkillArgs),
    /// Enable `fwt cd` by appending integration to your Bash/Zsh config
    Init(InitArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Shell to configure (defaults to the shell named by SHELL)
    #[arg(long, value_enum)]
    pub shell: Option<Shell>,

    /// Print the shell function without changing any files
    #[arg(long)]
    pub print: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
}

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Branch to check out or create
    pub branch: String,

    /// Sparse cone profile (defaults to FWT_CONE_DEFAULT or "default")
    #[arg(long, value_name = "NAME", conflicts_with_all = ["full", "cow"])]
    pub cone: Option<String>,

    /// Create a normal full worktree
    #[arg(long, conflicts_with = "cow")]
    pub full: bool,

    /// Create a full APFS copy-on-write clone
    #[arg(long)]
    pub cow: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Emit stable machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct BranchArgs {
    /// Branch whose checkout should be selected
    pub branch: String,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Branch whose checkout should be removed (the branch itself is kept)
    pub branch: String,

    /// Permanently discard uncommitted/untracked files in a linked worktree
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ConeArgs {
    #[command(subcommand)]
    pub command: ConeCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConeCommand {
    /// List profiles, provenance, and staleness state
    Ls(ListArgs),
    /// Define a manual cone profile
    Set(ConeSetArgs),
    /// Derive a cone from the transitive Bazel dependency graph
    Derive(ConeDeriveArgs),
}

#[derive(Debug, Args)]
pub struct ConeSetArgs {
    /// Profile name
    pub name: String,

    /// Description stored with the profile
    #[arg(long)]
    pub description: Option<String>,

    /// Directories included by the cone
    #[arg(required = true, num_args = 1..)]
    pub dirs: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ConeDeriveArgs {
    /// Profile name
    pub name: String,

    /// Bazel target expression, for example //service/...
    pub target: String,

    /// Description stored with the profile
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Install or refresh the skill embedded in this binary
    Install(SkillInstallArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Agent {
    ClaudeCode,
}

#[derive(Debug, Args)]
pub struct SkillInstallArgs {
    /// Agent harness to install for (v0 supports claude-code)
    #[arg(long, value_enum)]
    pub agent: Agent,
}

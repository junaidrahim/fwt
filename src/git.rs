use std::{
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde::Serialize;

use crate::error::{FwtError, Result};

#[derive(Clone, Debug)]
pub struct RepoContext {
    pub root: PathBuf,
    pub main: PathBuf,
    pub repo: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Clone, Debug)]
pub enum BranchStart {
    Local,
    Remote(String),
    New,
}

pub fn context() -> Result<RepoContext> {
    optional_context()?.ok_or_else(|| FwtError::Validation("not a git repository".to_owned()))
}

pub fn optional_context() -> Result<Option<RepoContext>> {
    let cwd =
        std::env::current_dir().map_err(|error| FwtError::io("read current directory", error))?;
    context_at(&cwd)
}

pub fn context_at(path: &Path) -> Result<Option<RepoContext>> {
    let root_output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if !root_output.status.success() {
        let diagnostic = String::from_utf8_lossy(&root_output.stderr);
        return if diagnostic.contains("not a git repository") {
            Ok(None)
        } else {
            Err(FwtError::underlying(
                "git rev-parse --show-toplevel",
                root_output.status,
                &root_output.stderr,
            ))
        };
    }
    let root = output_path(&root_output, "git rev-parse --show-toplevel")?;

    let common_output = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if !common_output.status.success() {
        return Err(FwtError::underlying(
            "git rev-parse --git-common-dir",
            common_output.status,
            &common_output.stderr,
        ));
    }
    let common = output_path(&common_output, "git rev-parse --git-common-dir")?;
    let main = if common.file_name() == Some(OsStr::new(".git")) {
        common.parent().map(Path::to_owned).ok_or_else(|| {
            FwtError::Validation("git returned an invalid common directory".to_owned())
        })?
    } else {
        // Bare repositories are not a supported source, but retaining the root here
        // produces a useful validation error in the operation that needs a checkout.
        root.clone()
    };
    let main = canonical(&main)?;
    let root = canonical(&root)?;
    let repo = main
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| FwtError::Validation("repository name is not valid UTF-8".to_owned()))?
        .to_owned();
    Ok(Some(RepoContext { root, main, repo }))
}

pub fn canonical(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path)
        .map_err(|error| FwtError::io(format!("resolve {}", path.display()), error))
}

pub fn assert_worktree_config(main: &Path) -> Result<()> {
    run_git(main, ["config", "extensions.worktreeConfig", "true"])
}

pub fn validate_branch(main: &Path, branch: &str) -> Result<()> {
    if branch.is_empty()
        || Path::new(branch).is_absolute()
        || Path::new(branch)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(FwtError::Validation(format!(
            "invalid branch name '{branch}'"
        )));
    }
    let output = Command::new("git")
        .args(["-C"])
        .arg(main)
        .args(["check-ref-format", "--branch", branch])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(FwtError::Validation(format!(
            "invalid branch name '{branch}'"
        )))
    }
}

pub fn branch_start(main: &Path, branch: &str) -> Result<BranchStart> {
    let local = Command::new("git")
        .args(["-C"])
        .arg(main)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .status()
        .map_err(|error| FwtError::command_start("git", error))?;
    if local.success() {
        return Ok(BranchStart::Local);
    } else if local.code() != Some(1) {
        return Err(FwtError::Underlying {
            program: "git show-ref".to_owned(),
            status: crate::error::StatusDisplay(local.code()),
            message: "could not inspect local branches".to_owned(),
        });
    }

    let output = git_output(
        main,
        ["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
    )?;
    let remotes: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|reference| {
            reference.split_once('/').is_some_and(|(_, remote_branch)| {
                remote_branch == branch && remote_branch != "HEAD"
            })
        })
        .map(str::to_owned)
        .collect();
    if remotes.len() == 1 {
        Ok(BranchStart::Remote(remotes[0].clone()))
    } else {
        if remotes.len() > 1 {
            eprintln!(
                "fwt: note: '{branch}' exists on multiple remotes; creating it from the current HEAD"
            );
        }
        Ok(BranchStart::New)
    }
}

pub fn add_worktree(main: &Path, target: &Path, branch: &str) -> Result<()> {
    let start = branch_start(main, branch)?;
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(main)
        .args(["worktree", "add", "--no-checkout"]);
    match start {
        BranchStart::Local => {
            command.arg(target).arg(branch);
        }
        BranchStart::Remote(remote) => {
            command
                .args(["--track", "-b"])
                .arg(branch)
                .arg(target)
                .arg(remote);
        }
        BranchStart::New => {
            command.args(["-b"]).arg(branch).arg(target);
        }
    }
    run_command(command, "git worktree add")
}

pub fn checkout_clone(repo: &Path, branch: &str) -> Result<()> {
    match branch_start(repo, branch)? {
        BranchStart::Local => run_git(repo, ["checkout", branch]),
        BranchStart::Remote(remote) => {
            let mut command = Command::new("git");
            command
                .args(["-C"])
                .arg(repo)
                .args(["checkout", "--track", "-b", branch])
                .arg(remote);
            run_command(command, "git checkout")
        }
        BranchStart::New => run_git(repo, ["checkout", "-b", branch]),
    }
}

pub fn sparse_checkout(target: &Path, dirs: &[String]) -> Result<()> {
    run_git(
        target,
        ["sparse-checkout", "init", "--cone", "--sparse-index"],
    )?;
    let mut input = dirs.join("\n");
    input.push('\n');
    run_git_with_input(target, ["sparse-checkout", "set", "--stdin"], &input)?;
    run_git(target, ["checkout"])
}

pub fn full_checkout(target: &Path) -> Result<()> {
    // Older repositories may keep sparse settings in shared config. Explicit
    // worktree overrides make --full independent without changing the source.
    run_git(
        target,
        ["config", "--worktree", "core.sparseCheckout", "false"],
    )?;
    run_git(
        target,
        ["config", "--worktree", "core.sparseCheckoutCone", "false"],
    )?;
    run_git(target, ["config", "--worktree", "index.sparse", "false"])?;
    run_git(target, ["checkout"])
}

pub fn remove_worktree(main: &Path, target: &Path, force: bool) -> Result<()> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(main).args(["worktree", "remove"]);
    if force {
        command.arg("--force");
    }
    command.arg(target);
    run_command(command, "git worktree remove")
}

pub fn cleanup_failed_worktree(main: &Path, target: &Path) {
    let _ = Command::new("git")
        .args(["-C"])
        .arg(main)
        .args(["worktree", "remove", "--force"])
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
}

pub fn list_worktrees(main: &Path) -> Result<Vec<GitWorktree>> {
    let output = git_output(main, ["worktree", "list", "--porcelain", "-z"])?;
    parse_worktree_porcelain(&output.stdout)
}

pub fn current_branch(repo: &Path) -> Result<Option<String>> {
    let output = git_output(repo, ["branch", "--show-current"])?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!branch.is_empty()).then_some(branch))
}

pub fn head(repo: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

pub fn remote_url(repo: &Path, remote: &str) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["remote", "get-url", remote])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(value);
    if path.exists() {
        Ok(Some(canonical(&path)?))
    } else {
        Ok(None)
    }
}

pub fn is_dirty(repo: &Path) -> Result<bool> {
    let output = git_output(repo, ["status", "--porcelain"])?;
    Ok(!output.stdout.is_empty())
}

pub fn is_sparse(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["config", "--worktree", "--bool", "core.sparseCheckout"])
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(FwtError::underlying(
            "git config --worktree core.sparseCheckout",
            output.status,
            &output.stderr,
        ))
    }
}

pub fn run_git<I, S>(repo: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(args);
    run_command(command, "git")
}

pub fn git_output<I, S>(repo: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| FwtError::command_start("git", error))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(FwtError::underlying("git", output.status, &output.stderr))
    }
}

pub fn run_command(mut command: Command, program: &str) -> Result<()> {
    let status = command
        .status()
        .map_err(|error| FwtError::command_start(program, error))?;
    if status.success() {
        Ok(())
    } else {
        Err(FwtError::Underlying {
            program: program.to_owned(),
            status: crate::error::StatusDisplay(status.code()),
            message: "see the command output above".to_owned(),
        })
    }
}

fn run_git_with_input<I, S>(repo: &Path, args: I, input: &str) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| FwtError::command_start("git", error))?;
    child
        .stdin
        .take()
        .ok_or_else(|| FwtError::Validation("could not open git stdin".to_owned()))?
        .write_all(input.as_bytes())
        .map_err(|error| {
            FwtError::underlying_message("git sparse-checkout set", error.to_string())
        })?;
    let status = child
        .wait()
        .map_err(|error| FwtError::underlying_message("git", error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(FwtError::Underlying {
            program: "git sparse-checkout set".to_owned(),
            status: crate::error::StatusDisplay(status.code()),
            message: "see the command output above".to_owned(),
        })
    }
}

fn output_path(output: &Output, command: &str) -> Result<PathBuf> {
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        Err(FwtError::Validation(format!(
            "{command} returned an empty path"
        )))
    } else {
        Ok(PathBuf::from(value))
    }
}

fn parse_worktree_porcelain(bytes: &[u8]) -> Result<Vec<GitWorktree>> {
    let mut worktrees = Vec::new();
    let mut path = None;
    let mut branch = None;
    let mut head = None;
    let mut locked = false;
    let mut prunable = false;

    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(path) = path.take() {
                worktrees.push(GitWorktree {
                    path,
                    branch: branch.take(),
                    head: head.take(),
                    locked,
                    prunable,
                });
            }
            locked = false;
            prunable = false;
            continue;
        }
        let field = String::from_utf8_lossy(field);
        if let Some(value) = field.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
        } else if let Some(value) = field.strip_prefix("HEAD ") {
            head = Some(value.to_owned());
        } else if let Some(value) = field.strip_prefix("branch refs/heads/") {
            branch = Some(value.to_owned());
        } else if field == "locked" || field.starts_with("locked ") {
            locked = true;
        } else if field == "prunable" || field.starts_with("prunable ") {
            prunable = true;
        }
    }
    if path.is_some() {
        return Err(FwtError::Validation(
            "git returned malformed worktree metadata".to_owned(),
        ));
    }
    Ok(worktrees)
}

#[cfg(test)]
mod tests {
    use super::parse_worktree_porcelain;

    #[test]
    fn parses_nul_delimited_worktrees() {
        let input = b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/wt\0HEAD def\0detached\0locked reason\0\0";
        let parsed = parse_worktree_porcelain(input).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert_eq!(parsed[1].branch, None);
        assert!(parsed[1].locked);
    }
}

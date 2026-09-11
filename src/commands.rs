use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use chrono::Utc;

use crate::{
    cli::{
        Agent, BranchArgs, ConeCommand, ListArgs, NewArgs, RemoveArgs, SkillCommand,
        SkillInstallArgs,
    },
    cone,
    error::{FwtError, Result},
    git,
    listing::{self, CheckoutKind},
    registry::{CloneRecord, RegistryStore},
    settings::Settings,
};

const CLAUDE_SKILL: &str = include_str!("../assets/claude-code/SKILL.md");

pub fn new(settings: &Settings, args: NewArgs) -> Result<()> {
    let context = listing::operation_context(settings)?;
    git::validate_branch(&context.main, &args.branch)?;
    let target = target_path(settings, &context.repo, &args.branch)?;
    if args.cow && target.starts_with(&context.main) {
        return Err(FwtError::Validation("--cow destination must be outside the source checkout; set FWT_BASE to a sibling directory".to_owned()));
    }
    if target
        .try_exists()
        .map_err(|error| FwtError::io("inspect worktree target", error))?
    {
        if target.join(".git").exists() {
            let existing = git::context_at(&target)?;
            let expected_main = if args.cow {
                git::remote_url(&target, "local")?
            } else {
                existing.as_ref().map(|repo| repo.main.clone())
            };
            if expected_main.as_ref() == Some(&context.main)
                && git::current_branch(&target)?.as_deref() == Some(&args.branch)
                && target.join(".git").is_dir() == args.cow
            {
                println!("exists: {} (checkout kept as-is)", target.display());
                return Ok(());
            }
        }
        return Err(FwtError::Validation(format!(
            "{} already exists but is not the requested branch/repository/checkout kind",
            target.display()
        )));
    }
    let profile = if args.full || args.cow {
        None
    } else {
        let name = args.cone.as_deref().unwrap_or(&settings.default_cone);
        Some(cone::load(settings, &context.repo, name)?)
    };

    fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| FwtError::Validation("worktree path has no parent".to_owned()))?,
    )
    .map_err(|error| FwtError::io(format!("create parent of {}", target.display()), error))?;
    if args.cow {
        return new_cow(settings, &context, &target, &args.branch);
    }
    git::assert_worktree_config(&context.main)?;

    println!("creating worktree (no checkout): {}", target.display());
    // Git owns rollback of a failed add. Another process may have created the
    // destination in the meantime, so only clean up after our add succeeds.
    git::add_worktree(&context.root, &target, &args.branch)?;

    let checkout = if let Some(profile) = &profile {
        println!(
            "sparse checkout: cone '{}' ({} dirs)",
            profile.name,
            profile.dirs.len()
        );
        git::sparse_checkout(&target, &profile.dirs)
    } else {
        println!("full checkout (the slow path)");
        git::full_checkout(&target)
    };
    if let Err(error) = checkout {
        eprintln!("fwt: checkout failed; removing {}", target.display());
        git::cleanup_failed_worktree(&context.main, &target);
        return Err(error);
    }

    seed_local_state(settings, &context.main, &target);
    println!("ready: {}", target.display());
    Ok(())
}

pub fn list(settings: &Settings, args: ListArgs) -> Result<()> {
    let list = listing::collect(settings)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&list).map_err(|source| FwtError::Json {
                context: "serialize checkout list".to_owned(),
                source,
            })?
        );
    } else {
        listing::print_human(&list);
    }
    Ok(())
}

pub fn cd(settings: &Settings, args: BranchArgs) -> Result<()> {
    let path = listing::resolve(settings, &args.branch)?;
    println!("{}", path.display());
    Ok(())
}

pub fn remove(settings: &Settings, args: RemoveArgs) -> Result<()> {
    let entry = listing::resolve_entry(settings, &args.branch)?;
    match entry.kind {
        CheckoutKind::Worktree => {
            let source = entry.source.as_deref().ok_or_else(|| {
                FwtError::Validation(format!(
                    "worktree {} has no source repository metadata",
                    entry.path.display()
                ))
            })?;
            if entry.path == source {
                return Err(FwtError::Validation(format!(
                    "refusing to remove the main checkout {}",
                    entry.path.display()
                )));
            }
            git::remove_worktree(source, &entry.path, args.force)?;
            RegistryStore::new(settings.registry_path.clone()).remove(&entry.path)?;
            println!("removed worktree {}", entry.path.display());
        }
        CheckoutKind::CowClone => {
            if git::list_worktrees(&entry.path)?.len() > 1 {
                return Err(FwtError::Validation(format!(
                    "{} owns linked worktrees; remove those worktrees before trashing the clone",
                    entry.path.display()
                )));
            }
            let trashed = move_to_trash(settings, &entry.path)?;
            RegistryStore::new(settings.registry_path.clone()).remove(&entry.path)?;
            prune_empty_branch_parents(&entry.path, &settings.base);
            println!(
                "trashed clone {} (recoverable at {})",
                entry.path.display(),
                trashed.display()
            );
        }
    }
    Ok(())
}

pub fn cone(settings: &Settings, command: ConeCommand) -> Result<()> {
    let context = listing::operation_context(settings)?;
    match command {
        ConeCommand::Ls(args) => {
            let cones = cone::list(settings, &context.repo)?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&cones).map_err(|source| FwtError::Json {
                        context: "serialize cone list".to_owned(),
                        source,
                    })?
                );
            } else if cones.is_empty() {
                println!("no cones defined for {}", context.repo);
            } else {
                let width = cones
                    .iter()
                    .map(|profile| profile.name.len())
                    .chain(std::iter::once("NAME".len()))
                    .max()
                    .unwrap_or(4);
                println!(
                    "{:<width$}  DIRS  SOURCE  STALENESS   DERIVED AT                  TARGET",
                    "NAME"
                );
                for profile in cones {
                    println!(
                        "{:<width$}  {:>4}  {:<6}  {:<11}  {:<26}  {}",
                        profile.name,
                        profile.dirs,
                        match profile.source {
                            cone::ConeSource::Manual => "manual",
                            cone::ConeSource::Bazel => "bazel",
                        },
                        profile.staleness,
                        profile
                            .derived_at
                            .map(|timestamp| timestamp.to_rfc3339())
                            .unwrap_or_else(|| "-".to_owned()),
                        profile.bazel_target.as_deref().unwrap_or("-")
                    );
                }
            }
        }
        ConeCommand::Set(args) => {
            let count = args.dirs.len();
            let path = cone::set_manual(
                settings,
                &context.repo,
                &args.name,
                args.description,
                args.dirs,
            )?;
            let profile = cone::load(settings, &context.repo, &args.name)?;
            println!(
                "wrote {} dirs to {}",
                profile.dirs.len().min(count),
                path.display()
            );
        }
        ConeCommand::Derive(args) => {
            let (path, count) = cone::derive(
                settings,
                &context.root,
                &context.repo,
                &args.name,
                &args.target,
                args.description,
            )?;
            println!("wrote {count} packages to {}", path.display());
        }
    }
    Ok(())
}

pub fn tune(settings: &Settings) -> Result<()> {
    let context = listing::operation_context(settings)?;
    let root = &context.root;
    println!("tuning {}", root.display());
    for (key, value) in [
        ("extensions.worktreeConfig", "true"),
        ("index.version", "4"),
        ("core.untrackedCache", "true"),
        ("core.fsmonitor", "true"),
        ("core.commitGraph", "true"),
        ("fetch.writeCommitGraph", "true"),
        ("checkout.workers", "0"),
    ] {
        git::run_git(root, ["config", key, value])?;
    }
    println!("rewriting index as v4");
    git::run_git(root, ["update-index", "--index-version", "4"])?;
    println!("writing commit graph");
    git::run_git(
        root,
        ["commit-graph", "write", "--reachable", "--changed-paths"],
    )?;
    println!("enabling background maintenance");
    git::run_git(root, ["maintenance", "start"])?;
    println!("done -- confirm with: git fsmonitor--daemon status");
    Ok(())
}

pub fn skill(settings: &Settings, command: SkillCommand) -> Result<()> {
    match command {
        SkillCommand::Install(args) => install_skill(settings, args),
    }
}

fn new_cow(
    settings: &Settings,
    context: &git::RepoContext,
    target: &Path,
    branch: &str,
) -> Result<()> {
    if git::is_sparse(&context.main)? {
        return Err(FwtError::Validation(
            "--cow requires a full source checkout; use --full for a full linked worktree"
                .to_owned(),
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| FwtError::Validation("clone path has no parent".to_owned()))?;
    if git::canonical(parent)?.starts_with(&context.main) {
        return Err(FwtError::Validation(
            "--cow destination must be outside the source checkout".to_owned(),
        ));
    }
    ensure_cow_supported(&context.main, parent)?;
    if git::is_dirty(&context.main)? {
        eprintln!(
            "fwt: warning: {} is dirty; its uncommitted changes will be carried onto '{}'",
            context.main.display(),
            branch
        );
    }
    println!(
        "APFS clone {} -> {} (copy-on-write)",
        context.main.display(),
        target.display()
    );
    // Reserve the destination before copying so a competing creator cannot
    // cause cp to nest into an existing checkout or rollback to delete it.
    fs::create_dir(target).map_err(|error| {
        FwtError::io(
            format!("reserve clone destination {}", target.display()),
            error,
        )
    })?;
    let output = Command::new("cp")
        .arg("-Rc")
        .arg(context.main.join("."))
        .arg(target)
        .output()
        .map_err(|error| FwtError::io("run cp -Rc", error))?;
    let diagnostics: Vec<_> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.contains("is a socket (not copied)"))
        .map(str::to_owned)
        .collect();
    if !diagnostics.is_empty() {
        eprintln!("{}", diagnostics.join("\n"));
    }
    let only_socket_warnings = !output.stderr.is_empty() && diagnostics.is_empty();
    if !target.join(".git").is_dir() || (!output.status.success() && !only_socket_warnings) {
        cleanup_failed_clone(target);
        return Err(FwtError::underlying(
            "cp -Rc",
            output.status,
            diagnostics.join("\n").as_bytes(),
        ));
    }

    let setup = (|| {
        let index_lock = target.join(".git/index.lock");
        if index_lock.exists() {
            fs::remove_file(&index_lock).map_err(|error| {
                FwtError::io(format!("remove stale {}", index_lock.display()), error)
            })?;
        }
        let inherited_worktrees = target.join(".git/worktrees");
        if inherited_worktrees.exists() {
            fs::remove_dir_all(&inherited_worktrees).map_err(|error| {
                FwtError::io(
                    format!("remove inherited {}", inherited_worktrees.display()),
                    error,
                )
            })?;
        }
        git::run_git(target, ["worktree", "prune"])?;
        configure_local_remote(target, &context.main)?;
        git::checkout_clone(target, branch)
    })();
    if let Err(error) = setup {
        cleanup_failed_clone(target);
        return Err(error);
    }

    let target = git::canonical(target)?;
    RegistryStore::new(settings.registry_path.clone()).add(CloneRecord {
        path: target.clone(),
        source: context.main.clone(),
        repo: context.repo.clone(),
        branch: branch.to_owned(),
        created_at: Utc::now(),
    })?;
    println!(
        "ready: {} (fresh Bazel analysis cache; action cache can remain shared)",
        target.display()
    );
    Ok(())
}

fn configure_local_remote(clone: &Path, source: &Path) -> Result<()> {
    // The cloned repository may itself have come from fwt and already contain
    // a `local` remote. It is clone-local state, so replacing it is unambiguous.
    let _ = Command::new("git")
        .args(["-C"])
        .arg(clone)
        .args(["remote", "remove", "local"])
        .output();
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(clone)
        .args(["remote", "add", "local"])
        .arg(source);
    git::run_command(command, "git remote add")
}

fn seed_local_state(settings: &Settings, source: &Path, target: &Path) {
    for item in &settings.seed {
        if item.is_absolute()
            || item.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            eprintln!(
                "fwt: warning: ignoring unsafe FWT_SEED entry '{}'",
                item.display()
            );
            continue;
        }
        let from = source.join(item);
        let to = target.join(item);
        if !from.exists() || to.symlink_metadata().is_ok() {
            continue;
        }
        // A tracked symlink in the destination must never redirect seed writes
        // outside the newly-created checkout.
        let mut ancestor = to.parent();
        let mut symlink_parent = false;
        while let Some(parent) = ancestor {
            if parent == target {
                break;
            }
            if parent
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                symlink_parent = true;
                break;
            }
            ancestor = parent.parent();
        }
        if symlink_parent {
            eprintln!(
                "fwt: warning: ignoring FWT_SEED entry '{}' with a symlink parent",
                item.display()
            );
            continue;
        }
        if let Some(parent) = to.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!(
                    "fwt: warning: could not create seed parent {}: {error}",
                    parent.display()
                );
                continue;
            }
        }
        let mut command = Command::new("cp");
        command.arg("-R");
        #[cfg(target_os = "macos")]
        command.arg("-c");
        match command.arg(&from).arg(&to).output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "fwt: warning: could not seed {}: {}",
                item.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => eprintln!("fwt: warning: could not seed {}: {error}", item.display()),
        }
    }
}

#[cfg(target_os = "macos")]
fn ensure_cow_supported(source: &Path, base: &Path) -> Result<()> {
    fs::create_dir_all(base)
        .map_err(|error| FwtError::io(format!("create {}", base.display()), error))?;
    let source_device = filesystem_device(source)?;
    let base_device = filesystem_device(base)?;
    if source_device != base_device {
        return Err(FwtError::Validation(format!(
            "--cow requires source and FWT_BASE on the same APFS volume ({source_device} != {base_device})"
        )));
    }
    let output = Command::new("diskutil")
        .args(["info", &source_device])
        .output()
        .map_err(|error| FwtError::io("run diskutil info", error))?;
    if !output.status.success() {
        return Err(FwtError::underlying(
            "diskutil info",
            output.status,
            &output.stderr,
        ));
    }
    let info = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    if !info.lines().any(|line| {
        (line.contains("type (bundle)") && line.ends_with("apfs"))
            || (line.contains("file system personality") && line.ends_with("apfs"))
    }) {
        return Err(FwtError::Validation(
            "--cow is supported only on APFS".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn filesystem_device(path: &Path) -> Result<String> {
    let output = Command::new("df")
        .arg("-P")
        .arg(path)
        .output()
        .map_err(|error| FwtError::io("run df -P", error))?;
    if !output.status.success() {
        return Err(FwtError::underlying("df -P", output.status, &output.stderr));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| FwtError::Validation("could not determine filesystem device".to_owned()))
}

#[cfg(not(target_os = "macos"))]
fn ensure_cow_supported(_source: &Path, _base: &Path) -> Result<()> {
    Err(FwtError::Validation(
        "--cow is supported only on macOS with APFS".to_owned(),
    ))
}

fn cleanup_failed_clone(target: &Path) {
    if target.exists() {
        let _ = fs::remove_dir_all(target);
    }
}

fn target_path(settings: &Settings, repo: &str, branch: &str) -> Result<PathBuf> {
    let target = settings.base.join(format!("{repo}@{branch}"));
    // Branch validation rules out '..', but this lexical guard keeps filesystem
    // targeting independent of Git's parser.
    if target.strip_prefix(&settings.base).is_err() || target == settings.base {
        return Err(FwtError::Validation(
            "worktree target escapes FWT_BASE".to_owned(),
        ));
    }
    let mut parent = target.parent();
    while let Some(path) = parent {
        if path == settings.base {
            break;
        }
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(FwtError::Validation(format!(
                "worktree parent {} is a symlink",
                path.display()
            )));
        }
        parent = path.parent();
    }
    Ok(target)
}

fn move_to_trash(settings: &Settings, path: &Path) -> Result<PathBuf> {
    let trash = settings.home.join(".Trash");
    fs::create_dir_all(&trash)
        .map_err(|error| FwtError::io(format!("create {}", trash.display()), error))?;
    let trash = git::canonical(&trash)?;
    let canonical_base = if settings.base.exists() {
        git::canonical(&settings.base)?
    } else {
        settings.base.clone()
    };
    let relative = path
        .strip_prefix(&canonical_base)
        .unwrap_or_else(|_| path.file_name().map(Path::new).unwrap_or(path));
    let base_name = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("__");
    let destination_name = format!(
        "{}-{}-{}",
        if base_name.is_empty() {
            "fwt-clone"
        } else {
            &base_name
        },
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        std::process::id()
    );
    let destination = trash.join(&destination_name);
    match fs::rename(path, &destination) {
        Ok(()) => Ok(destination),
        Err(error) if error.raw_os_error() == Some(18) => {
            // ~/.Trash can be on another device when FWT_BASE is an external
            // APFS volume. Keep deletion recoverable without copying and then
            // recursively deleting a potentially multi-gigabyte clone.
            let local_trash = canonical_base.join(".fwt-trash");
            fs::create_dir_all(&local_trash).map_err(|error| {
                FwtError::io(format!("create {}", local_trash.display()), error)
            })?;
            let fallback = local_trash.join(destination_name);
            fs::rename(path, &fallback).map_err(|error| {
                FwtError::io(
                    format!(
                        "move {} to local trash at {} (the clone was left untouched)",
                        path.display(),
                        fallback.display()
                    ),
                    error,
                )
            })?;
            Ok(fallback)
        }
        Err(error) => Err(FwtError::io(
            format!(
                "move {} to Trash at {} (the clone was left untouched)",
                path.display(),
                destination.display()
            ),
            error,
        )),
    }
}

fn prune_empty_branch_parents(path: &Path, base: &Path) {
    let canonical_base = fs::canonicalize(base).unwrap_or_else(|_| base.to_owned());
    let mut parent = path.parent();
    while let Some(directory) = parent {
        if directory == canonical_base || !directory.starts_with(&canonical_base) {
            break;
        }
        if fs::remove_dir(directory).is_err() {
            break;
        }
        parent = directory.parent();
    }
}

fn install_skill(settings: &Settings, args: SkillInstallArgs) -> Result<()> {
    let destination = match args.agent {
        Agent::ClaudeCode => settings.home.join(".claude/skills/fwt/SKILL.md"),
    };
    let version = env!("CARGO_PKG_VERSION");
    let rendered = CLAUDE_SKILL.replace("{{FWT_VERSION}}", version);
    if destination.is_file()
        && fs::read_to_string(&destination)
            .map_err(|error| FwtError::io(format!("read {}", destination.display()), error))?
            == rendered
    {
        println!("skill is already up to date: {}", destination.display());
        return Ok(());
    }
    let parent = destination.parent().expect("skill destination has parent");
    fs::create_dir_all(parent)
        .map_err(|error| FwtError::io(format!("create {}", parent.display()), error))?;
    let temporary = parent.join(format!(".SKILL.md.tmp.{}", std::process::id()));
    fs::write(&temporary, rendered)
        .map_err(|error| FwtError::io(format!("write {}", temporary.display()), error))?;
    fs::rename(&temporary, &destination)
        .map_err(|error| FwtError::io(format!("replace {}", destination.display()), error))?;
    println!("installed Claude Code skill: {}", destination.display());
    Ok(())
}

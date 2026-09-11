use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{
    error::{FwtError, Result},
    git::{self, RepoContext},
    registry::{CloneRecord, RegistryStore},
    settings::Settings,
};

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutKind {
    Worktree,
    CowClone,
}

impl std::fmt::Display for CheckoutKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Worktree => f.write_str("worktree"),
            Self::CowClone => f.write_str("cow-clone"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckoutEntry {
    pub kind: CheckoutKind,
    pub path: PathBuf,
    pub repo: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub source: Option<PathBuf>,
    pub locked: bool,
    pub prunable: bool,
    pub registered: bool,
    pub registered_branch: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct CheckoutList {
    pub repo: Option<String>,
    pub entries: Vec<CheckoutEntry>,
}

#[derive(Clone, Debug)]
struct CloneCandidate {
    path: PathBuf,
    source: Option<PathBuf>,
    repo: String,
    registered_branch: String,
    registered: bool,
    created_at: Option<DateTime<Utc>>,
}

pub fn effective_context(settings: &Settings) -> Result<RepoContext> {
    let raw = git::context()?;
    let records = RegistryStore::new(settings.registry_path.clone()).load()?;
    if let Some(record) = records.iter().find(|record| record.path == raw.main) {
        if let Some(source) = if record.source.is_dir() {
            git::context_at(&record.source)?
        } else {
            None
        } {
            return Ok(RepoContext {
                root: raw.root,
                main: source.main,
                repo: record.repo.clone(),
            });
        }
        return Ok(RepoContext {
            root: raw.root,
            main: raw.main,
            repo: record.repo.clone(),
        });
    }
    if raw.root == raw.main {
        if let Some(source_path) = git::remote_url(&raw.root, "local")? {
            if let Some(source) = git::context_at(&source_path)? {
                return Ok(RepoContext {
                    root: raw.root,
                    main: source.main,
                    repo: source.repo,
                });
            }
        }
    }
    Ok(raw)
}

/// Return the Git repository that commands should mutate while preserving the
/// logical repository name for a registered or legacy COW clone. Unlike
/// `effective_context`, this deliberately does not jump back to the clone's
/// source repository.
pub fn operation_context(settings: &Settings) -> Result<RepoContext> {
    let mut raw = git::context()?;
    let records = RegistryStore::new(settings.registry_path.clone()).load()?;
    if let Some(record) = records.iter().find(|record| record.path == raw.main) {
        raw.repo = record.repo.clone();
        return Ok(raw);
    }
    if raw.root == raw.main {
        if let Some(source_path) = git::remote_url(&raw.root, "local")? {
            if let Some(source) = git::context_at(&source_path)? {
                raw.repo = source.repo;
            }
        }
    }
    Ok(raw)
}

pub fn collect(settings: &Settings) -> Result<CheckoutList> {
    let store = RegistryStore::new(settings.registry_path.clone());
    let records = store.load()?;
    let raw_context = git::optional_context()?;
    let scope = match raw_context {
        Some(_) => Some(effective_context(settings)?),
        None => None,
    };

    let candidates = clone_candidates(settings, &records)?;
    let scoped_candidates: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| match &scope {
            Some(scope) => {
                candidate.path == scope.main
                    || match &candidate.source {
                        Some(source) => source == &scope.main,
                        None => candidate.repo == scope.repo,
                    }
            }
            None => true,
        })
        .collect();

    let mut sources = BTreeSet::new();
    if let Some(scope) = &scope {
        sources.insert(scope.main.clone());
    } else {
        for record in &records {
            if record.source.exists() {
                sources.insert(record.source.clone());
            }
        }
        for candidate in &scoped_candidates {
            if let Some(source) = &candidate.source {
                if source.exists() {
                    sources.insert(source.clone());
                }
            }
        }
    }
    // A COW clone is an independent repository and may itself own linked
    // worktrees. Enumerating it keeps the merged view complete without
    // misclassifying the clone's own root as a linked worktree below.
    let cow_paths: BTreeSet<_> = scoped_candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    let cow_repos: BTreeMap<_, _> = scoped_candidates
        .iter()
        .map(|candidate| (candidate.path.clone(), candidate.repo.clone()))
        .collect();
    sources.extend(cow_paths.iter().cloned());

    let mut by_path = BTreeMap::new();
    for source in sources {
        let Some(source_context) = git::context_at(&source)? else {
            continue;
        };
        let logical_repo = cow_repos
            .get(&source_context.main)
            .cloned()
            .unwrap_or_else(|| source_context.repo.clone());
        for worktree in git::list_worktrees(&source_context.main)? {
            let path = if worktree.path.exists() {
                git::canonical(&worktree.path)?
            } else {
                worktree.path
            };
            if cow_paths.contains(&path) {
                continue;
            }
            by_path.insert(
                path.clone(),
                CheckoutEntry {
                    kind: CheckoutKind::Worktree,
                    path,
                    repo: logical_repo.clone(),
                    branch: worktree.branch,
                    head: worktree.head,
                    source: Some(source_context.main.clone()),
                    locked: worktree.locked,
                    prunable: worktree.prunable,
                    registered: false,
                    registered_branch: None,
                    created_at: None,
                },
            );
        }
    }

    for candidate in scoped_candidates {
        let path = git::canonical(&candidate.path)?;
        if by_path.contains_key(&path) {
            continue;
        }
        by_path.insert(
            path.clone(),
            CheckoutEntry {
                kind: CheckoutKind::CowClone,
                branch: git::current_branch(&candidate.path)?,
                head: git::head(&candidate.path)?,
                path,
                repo: candidate.repo,
                source: candidate.source,
                locked: false,
                prunable: false,
                registered: candidate.registered,
                registered_branch: Some(candidate.registered_branch),
                created_at: candidate.created_at,
            },
        );
    }

    let mut entries: Vec<_> = by_path.into_values().collect();
    entries.sort_by(|left, right| {
        left.repo
            .cmp(&right.repo)
            .then_with(|| left.branch.cmp(&right.branch))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(CheckoutList {
        repo: scope.map(|scope| scope.repo),
        entries,
    })
}

pub fn resolve(settings: &Settings, branch: &str) -> Result<PathBuf> {
    Ok(resolve_entry(settings, branch)?.path)
}

pub fn resolve_entry(settings: &Settings, branch: &str) -> Result<CheckoutEntry> {
    let list = collect(settings)?;
    let matches: Vec<_> = list
        .entries
        .into_iter()
        .filter(|entry| {
            entry.branch.as_deref() == Some(branch)
                || entry.registered_branch.as_deref() == Some(branch)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(FwtError::Validation(format!(
            "no worktree or clone for branch '{branch}'"
        ))),
        [entry] => Ok(entry.clone()),
        entries => Err(FwtError::Validation(format!(
            "branch '{branch}' is ambiguous; it matches: {}",
            entries
                .iter()
                .map(|entry| entry.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

pub fn print_human(list: &CheckoutList) {
    if list.entries.is_empty() {
        match &list.repo {
            Some(repo) => println!("no worktrees or clones for {repo}"),
            None => println!("no registered worktrees or clones"),
        }
        return;
    }

    let branch_width = list
        .entries
        .iter()
        .map(|entry| entry.branch.as_deref().unwrap_or("(detached)").len())
        .chain(std::iter::once("BRANCH".len()))
        .max()
        .unwrap_or(6);
    let kind_width = "cow-clone".len();
    println!("{:<branch_width$}  {:<kind_width$}  PATH", "BRANCH", "KIND");
    for entry in &list.entries {
        let mut state = String::new();
        if entry.locked {
            state.push_str(" [locked]");
        }
        if entry.prunable {
            state.push_str(" [prunable]");
        }
        println!(
            "{:<branch_width$}  {:<kind_width$}  {}{}",
            entry.branch.as_deref().unwrap_or("(detached)"),
            entry.kind,
            entry.path.display(),
            state,
        );
    }
}

fn clone_candidates(settings: &Settings, records: &[CloneRecord]) -> Result<Vec<CloneCandidate>> {
    let mut candidates = BTreeMap::new();
    for record in records {
        if record.path.is_dir() && record.path.join(".git").is_dir() {
            let path = git::canonical(&record.path)?;
            candidates.insert(
                path.clone(),
                CloneCandidate {
                    path,
                    source: record.source.exists().then(|| record.source.clone()),
                    repo: record.repo.clone(),
                    registered_branch: record.branch.clone(),
                    registered: true,
                    created_at: Some(record.created_at),
                },
            );
        }
    }

    for (path, repo, registered_branch) in discover_clones(&settings.base)? {
        let path = git::canonical(&path)?;
        if candidates.contains_key(&path) {
            continue;
        }
        let source = git::remote_url(&path, "local")?;
        candidates.insert(
            path.clone(),
            CloneCandidate {
                path,
                source,
                repo,
                registered_branch,
                registered: false,
                created_at: None,
            },
        );
    }
    Ok(candidates.into_values().collect())
}

fn discover_clones(base: &Path) -> Result<Vec<(PathBuf, String, String)>> {
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(base)
        .map_err(|error| FwtError::io(format!("read {}", base.display()), error))?
    {
        let entry = entry.map_err(|error| FwtError::io("read FWT_BASE entry", error))?;
        if !entry
            .file_type()
            .map_err(|error| FwtError::io("read FWT_BASE file type", error))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((repo, first_branch_component)) = name.split_once('@') else {
            continue;
        };
        if repo.is_empty() || first_branch_component.is_empty() {
            continue;
        }
        discover_branch_path(
            &entry.path(),
            repo,
            first_branch_component.to_owned(),
            0,
            &mut result,
        )?;
    }
    Ok(result)
}

fn discover_branch_path(
    path: &Path,
    repo: &str,
    branch: String,
    depth: usize,
    result: &mut Vec<(PathBuf, String, String)>,
) -> Result<()> {
    if path.join(".git").is_dir() {
        result.push((path.to_owned(), repo.to_owned(), branch));
        return Ok(());
    }
    if path.join(".git").is_file() || depth >= 32 {
        return Ok(());
    }
    for entry in fs::read_dir(path)
        .map_err(|error| FwtError::io(format!("read {}", path.display()), error))?
    {
        let entry = entry.map_err(|error| FwtError::io("read branch path", error))?;
        if !entry
            .file_type()
            .map_err(|error| FwtError::io("read branch path type", error))?
            .is_dir()
            || entry.file_name() == OsStr::new(".git")
        {
            continue;
        }
        let Some(component) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        discover_branch_path(
            &entry.path(),
            repo,
            format!("{branch}/{component}"),
            depth + 1,
            result,
        )?;
    }
    Ok(())
}

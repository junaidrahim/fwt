use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    error::{FwtError, Result},
    git,
    settings::Settings,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConeSource {
    Manual,
    Bazel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConeProfile {
    pub name: String,
    pub description: Option<String>,
    pub source: ConeSource,
    pub bazel_target: Option<String>,
    pub derived_at: Option<DateTime<Utc>>,
    pub dirs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConeSummary {
    pub name: String,
    pub description: Option<String>,
    pub source: ConeSource,
    pub bazel_target: Option<String>,
    pub derived_at: Option<DateTime<Utc>>,
    pub dirs: usize,
    pub staleness: Staleness,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Staleness {
    NotApplicable,
    NotChecked,
}

impl std::fmt::Display for Staleness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotApplicable => f.write_str("n/a"),
            Self::NotChecked => f.write_str("not checked"),
        }
    }
}

pub fn set_manual(
    settings: &Settings,
    repo: &str,
    name: &str,
    description: Option<String>,
    dirs: Vec<String>,
) -> Result<PathBuf> {
    validate_profile_name(name)?;
    let profile = ConeProfile {
        name: name.to_owned(),
        description,
        source: ConeSource::Manual,
        bazel_target: None,
        derived_at: None,
        dirs: normalize_dirs(dirs)?,
    };
    write_profile(settings, repo, &profile)
}

pub fn derive(
    settings: &Settings,
    main: &Path,
    repo: &str,
    name: &str,
    target: &str,
    description: Option<String>,
) -> Result<(PathBuf, usize)> {
    validate_profile_name(name)?;
    if target.trim().is_empty() {
        return Err(FwtError::Validation(
            "bazel target must not be empty".to_owned(),
        ));
    }
    if git::is_sparse(main)? {
        return Err(FwtError::Validation(format!(
            "{} is sparse; cone derivation requires a full checkout",
            main.display()
        )));
    }

    eprintln!(
        "fwt: resolving the dependency closure of {target} from {} (this can take a few minutes)",
        main.display()
    );
    let query = format!("buildfiles(deps({target}))");
    let output = Command::new("bazel")
        .current_dir(main)
        .args(["query", &query, "--output", "package"])
        .output()
        .map_err(|error| FwtError::command_start("bazel", error))?;
    if !output.status.success() {
        return Err(FwtError::underlying(
            "bazel query",
            output.status,
            &output.stderr,
        ));
    }

    let dirs = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('@'))
        .map(|line| line.strip_prefix("//").unwrap_or(line).to_owned())
        .collect::<Vec<_>>();
    let dirs = normalize_dirs(dirs)?;
    if dirs.is_empty() {
        return Err(FwtError::underlying_message(
            "bazel query",
            "the query returned no local packages (wrong target or incomplete checkout)",
        ));
    }
    let count = dirs.len();
    let profile = ConeProfile {
        name: name.to_owned(),
        description: description.or_else(|| Some(format!("Bazel dependency closure for {target}"))),
        source: ConeSource::Bazel,
        bazel_target: Some(target.to_owned()),
        derived_at: Some(Utc::now()),
        dirs,
    };
    let path = write_profile(settings, repo, &profile)?;
    Ok((path, count))
}

pub fn load(settings: &Settings, repo: &str, name: &str) -> Result<ConeProfile> {
    validate_profile_name(name)?;
    let repo_dir = settings.cone_dir.join(repo);
    let yaml = repo_dir.join(format!("{name}.yaml"));
    if yaml.is_file() {
        return read_yaml(&yaml, Some(name));
    }
    let legacy = repo_dir.join(name);
    if legacy.is_file() {
        return migrate_legacy(&legacy, &yaml, name);
    }
    Err(FwtError::Validation(format!(
        "no cone '{name}' at {} (define it with `fwt cone set {name} <dir>...`)",
        yaml.display()
    )))
}

pub fn list(settings: &Settings, repo: &str) -> Result<Vec<ConeSummary>> {
    let repo_dir = settings.cone_dir.join(repo);
    if !repo_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&repo_dir)
        .map_err(|error| FwtError::io(format!("read {}", repo_dir.display()), error))?
    {
        let entry = entry.map_err(|error| FwtError::io("read cone directory entry", error))?;
        if !entry
            .file_type()
            .map_err(|error| FwtError::io("read cone file type", error))?
            .is_file()
        {
            continue;
        }
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| FwtError::Validation("cone filename is not valid UTF-8".to_owned()))?;
        if filename.contains(".tmp.") {
            continue;
        }
        if let Some(name) = filename.strip_suffix(".yaml") {
            names.insert(name.to_owned());
        } else if valid_profile_name(&filename) {
            names.insert(filename);
        }
    }

    names
        .into_iter()
        .map(|name| {
            let profile = load(settings, repo, &name)?;
            validate_profile(&profile, Some(&name))?;
            Ok(ConeSummary {
                name: profile.name,
                description: profile.description,
                staleness: match profile.source {
                    ConeSource::Manual => Staleness::NotApplicable,
                    ConeSource::Bazel => Staleness::NotChecked,
                },
                source: profile.source,
                bazel_target: profile.bazel_target,
                derived_at: profile.derived_at,
                dirs: profile.dirs.len(),
            })
        })
        .collect()
}

fn write_profile(settings: &Settings, repo: &str, profile: &ConeProfile) -> Result<PathBuf> {
    validate_profile(profile, Some(&profile.name))?;
    let repo_dir = settings.cone_dir.join(repo);
    fs::create_dir_all(&repo_dir)
        .map_err(|error| FwtError::io(format!("create {}", repo_dir.display()), error))?;
    let path = repo_dir.join(format!("{}.yaml", profile.name));
    let yaml = serde_yaml::to_string(profile).map_err(|source| FwtError::Yaml {
        context: format!("serialize {}", path.display()),
        source,
    })?;
    atomic_write(&path, yaml.as_bytes())?;
    Ok(path)
}

fn read_yaml(path: &Path, expected_name: Option<&str>) -> Result<ConeProfile> {
    let bytes =
        fs::read(path).map_err(|error| FwtError::io(format!("read {}", path.display()), error))?;
    let profile: ConeProfile = serde_yaml::from_slice(&bytes).map_err(|source| FwtError::Yaml {
        context: format!("parse {}", path.display()),
        source,
    })?;
    validate_profile(&profile, expected_name)?;
    Ok(profile)
}

fn migrate_legacy(legacy: &Path, yaml: &Path, name: &str) -> Result<ConeProfile> {
    let contents = fs::read_to_string(legacy)
        .map_err(|error| FwtError::io(format!("read {}", legacy.display()), error))?;
    let dirs = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let profile = ConeProfile {
        name: name.to_owned(),
        description: Some("Migrated from the legacy flat cone format".to_owned()),
        source: ConeSource::Manual,
        bazel_target: None,
        derived_at: None,
        dirs: normalize_dirs(dirs)?,
    };
    let serialized = serde_yaml::to_string(&profile).map_err(|source| FwtError::Yaml {
        context: format!("serialize {}", yaml.display()),
        source,
    })?;
    atomic_write(yaml, serialized.as_bytes())?;
    fs::remove_file(legacy)
        .map_err(|error| FwtError::io(format!("remove migrated {}", legacy.display()), error))?;
    eprintln!("fwt: migrated legacy cone '{}' to {}", name, yaml.display());
    Ok(profile)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        FwtError::Validation(format!("{} has no parent directory", path.display()))
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| FwtError::io(format!("create {}", parent.display()), error))?;
    let filename = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| FwtError::Validation(format!("{} is not valid UTF-8", path.display())))?;
    let temporary = parent.join(format!(".{filename}.tmp.{}", std::process::id()));
    fs::write(&temporary, bytes)
        .map_err(|error| FwtError::io(format!("write {}", temporary.display()), error))?;
    fs::rename(&temporary, path)
        .map_err(|error| FwtError::io(format!("replace {}", path.display()), error))
}

fn validate_profile(profile: &ConeProfile, expected_name: Option<&str>) -> Result<()> {
    validate_profile_name(&profile.name)?;
    if let Some(expected_name) = expected_name {
        if profile.name != expected_name {
            return Err(FwtError::Validation(format!(
                "cone '{}' declares name '{}'",
                expected_name, profile.name
            )));
        }
    }
    if profile.dirs.is_empty() {
        return Err(FwtError::Validation(format!(
            "cone '{}' contains no directories",
            profile.name
        )));
    }
    for dir in &profile.dirs {
        normalize_dir(dir)?;
    }
    match profile.source {
        ConeSource::Manual if profile.bazel_target.is_some() || profile.derived_at.is_some() => {
            Err(FwtError::Validation(format!(
                "manual cone '{}' cannot have bazel_target or derived_at",
                profile.name
            )))
        }
        ConeSource::Bazel if profile.bazel_target.is_none() || profile.derived_at.is_none() => {
            Err(FwtError::Validation(format!(
                "bazel cone '{}' requires bazel_target and derived_at",
                profile.name
            )))
        }
        _ => Ok(()),
    }
}

fn normalize_dirs(dirs: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = BTreeSet::new();
    for dir in dirs {
        normalized.insert(normalize_dir(&dir)?);
    }
    if normalized.is_empty() {
        return Err(FwtError::Validation(
            "a cone must include at least one directory".to_owned(),
        ));
    }
    Ok(normalized.into_iter().collect())
}

fn normalize_dir(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_start_matches("./").trim_end_matches('/');
    let path = Path::new(trimmed);
    if trimmed.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(FwtError::Validation(format!(
            "invalid cone directory '{value}' (directories must be relative to the repository)"
        )));
    }
    Ok(trimmed.to_owned())
}

fn validate_profile_name(name: &str) -> Result<()> {
    if valid_profile_name(name) {
        Ok(())
    } else {
        Err(FwtError::Validation(format!(
            "invalid cone name '{name}' (use letters, digits, '.', '_' or '-')"
        )))
    }
}

fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

#[cfg(test)]
mod tests {
    use super::{ConeSource, normalize_dirs};

    #[test]
    fn normalizes_and_deduplicates_dirs() {
        let dirs =
            normalize_dirs(vec!["./platform/utils/".into(), "platform/utils".into()]).unwrap();
        assert_eq!(dirs, ["platform/utils"]);
    }

    #[test]
    fn source_serializes_as_lowercase() {
        assert_eq!(
            serde_yaml::to_string(&ConeSource::Bazel).unwrap(),
            "bazel\n"
        );
    }
}

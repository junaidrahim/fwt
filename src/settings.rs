use std::{
    env,
    path::{Path, PathBuf},
};

use crate::error::{FwtError, Result};

const DEFAULT_SEED: &[&str] = &[".env", ".claude", ".bazelbsp", ".npmrc", ".vscode"];

#[derive(Clone, Debug)]
pub struct Settings {
    pub home: PathBuf,
    pub base: PathBuf,
    pub cone_dir: PathBuf,
    pub default_cone: String,
    pub seed: Vec<PathBuf>,
    pub registry_path: PathBuf,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| FwtError::Validation("HOME is not set".to_owned()))?;

        let base = env_path("FWT_BASE", &home.join("fivetran/worktrees"), &home);
        let cone_dir = env_path("FWT_CONE_DIR", &home.join(".config/fivetran-cones"), &home);
        let default_cone = env::var("FWT_CONE_DEFAULT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "default".to_owned());
        let seed = env::var("FWT_SEED")
            .ok()
            .map(|value| parse_seed(&value))
            .unwrap_or_else(|| DEFAULT_SEED.iter().map(PathBuf::from).collect());

        Ok(Self {
            registry_path: home.join(".config/fwt/clones.json"),
            home,
            base,
            cone_dir,
            default_cone,
            seed,
        })
    }
}

fn env_path(name: &str, default: &Path, home: &Path) -> PathBuf {
    let Some(value) = env::var_os(name).filter(|value| !value.is_empty()) else {
        return default.to_path_buf();
    };
    let path = PathBuf::from(value);
    if path == std::path::Path::new("~") {
        home.to_owned()
    } else if let Ok(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        path
    }
}

fn parse_seed(value: &str) -> Vec<PathBuf> {
    value
        .split(|character: char| character.is_whitespace() || character == ',' || character == ':')
        .filter(|item| !item.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_seed;

    #[test]
    fn seed_accepts_shell_style_and_path_style_separators() {
        let seed = parse_seed(".env .claude,.vscode:.npmrc");
        assert_eq!(seed.len(), 4);
        assert_eq!(seed[2].to_string_lossy(), ".vscode");
    }
}

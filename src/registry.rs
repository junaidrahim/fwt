use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::{FwtError, Result};

const REGISTRY_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CloneRecord {
    pub path: PathBuf,
    pub source: PathBuf,
    pub repo: String,
    pub branch: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Registry {
    version: u32,
    clones: Vec<CloneRecord>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            clones: Vec::new(),
        }
    }
}

pub struct RegistryStore {
    path: PathBuf,
}

impl RegistryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<CloneRecord>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let lock = self.open_lock()?;
        FileExt::lock_shared(&lock).map_err(|error| FwtError::io("lock clone registry", error))?;
        let registry = self.read_unlocked()?;
        FileExt::unlock(&lock).map_err(|error| FwtError::io("unlock clone registry", error))?;
        Ok(registry.clones)
    }

    pub fn add(&self, record: CloneRecord) -> Result<()> {
        self.mutate(|registry| {
            registry.clones.retain(|entry| entry.path != record.path);
            registry.clones.push(record);
            registry
                .clones
                .sort_by(|left, right| left.path.cmp(&right.path));
        })
    }

    pub fn remove(&self, path: &Path) -> Result<()> {
        self.mutate(|registry| registry.clones.retain(|entry| entry.path != path))
    }

    fn mutate(&self, update: impl FnOnce(&mut Registry)) -> Result<()> {
        let parent = self.path.parent().ok_or_else(|| {
            FwtError::Validation(format!("{} has no parent", self.path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| FwtError::io(format!("create {}", parent.display()), error))?;
        let lock = self.open_lock()?;
        FileExt::lock_exclusive(&lock)
            .map_err(|error| FwtError::io("lock clone registry", error))?;
        let mut registry = self.read_unlocked()?;
        update(&mut registry);
        self.write_unlocked(&registry)?;
        FileExt::unlock(&lock).map_err(|error| FwtError::io("unlock clone registry", error))
    }

    fn read_unlocked(&self) -> Result<Registry> {
        if !self.path.exists() {
            return Ok(Registry::default());
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| FwtError::io(format!("read {}", self.path.display()), error))?;
        let registry: Registry =
            serde_json::from_slice(&bytes).map_err(|source| FwtError::Json {
                context: format!("parse {}", self.path.display()),
                source,
            })?;
        if registry.version != REGISTRY_VERSION {
            return Err(FwtError::Validation(format!(
                "unsupported clone registry version {} in {}",
                registry.version,
                self.path.display()
            )));
        }
        Ok(registry)
    }

    fn write_unlocked(&self, registry: &Registry) -> Result<()> {
        let parent = self.path.parent().expect("registry path has a parent");
        let temporary = parent.join(format!(".clones.json.tmp.{}", std::process::id()));
        let mut bytes = serde_json::to_vec_pretty(registry).map_err(|source| FwtError::Json {
            context: "serialize clone registry".to_owned(),
            source,
        })?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)
            .map_err(|error| FwtError::io(format!("write {}", temporary.display()), error))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| FwtError::io(format!("replace {}", self.path.display()), error))
    }

    fn open_lock(&self) -> Result<File> {
        let parent = self.path.parent().ok_or_else(|| {
            FwtError::Validation(format!("{} has no parent", self.path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| FwtError::io(format!("create {}", parent.display()), error))?;
        let lock_path = parent.join("clones.lock");
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| FwtError::io(format!("open {}", lock_path.display()), error))
    }
}

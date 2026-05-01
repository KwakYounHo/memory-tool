use anyhow::{Result, anyhow};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDirs {
    pub root: PathBuf,
    pub inbox: PathBuf,
    pub corpus: PathBuf,
    pub db: PathBuf,
}

impl DataDirs {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();

        Self {
            inbox: root.join("inbox"),
            corpus: root.join("corpus"),
            db: root.join("memory.sqlite"),
            root,
        }
    }
}

pub fn data_dirs() -> Result<DataDirs> {
    Ok(DataDirs::from_root(data_root()?))
}

pub fn ensure_data_dirs(dirs: &DataDirs) -> Result<()> {
    create_dir_all(&dirs.root)?;
    create_dir_all(&dirs.inbox)?;
    create_dir_all(&dirs.corpus)?;
    Ok(())
}

fn data_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("MEMORY_TOOL_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("memory-tool"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".local/share/memory-tool"))
}

fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|error| anyhow!("create directory {}: {error}", path.display()))
}

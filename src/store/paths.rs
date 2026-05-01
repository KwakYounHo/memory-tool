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
    data_root_from_env(
        env::var_os("MEMORY_TOOL_DATA_DIR").map(PathBuf::from),
        env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn data_root_from_env(
    memory_tool_data_dir: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = memory_tool_data_dir {
        return Ok(path);
    }

    if let Some(path) = xdg_data_home {
        return Ok(path.join("memory-tool"));
    }

    let home = home.ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(home.join(".local/share/memory-tool"))
}

fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|error| anyhow!("create directory {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn data_dirs_from_root_derives_child_paths() {
        let dirs = DataDirs::from_root("/tmp/memory-tool-test");

        assert_eq!(dirs.root, PathBuf::from("/tmp/memory-tool-test"));
        assert_eq!(dirs.inbox, PathBuf::from("/tmp/memory-tool-test/inbox"));
        assert_eq!(dirs.corpus, PathBuf::from("/tmp/memory-tool-test/corpus"));
        assert_eq!(
            dirs.db,
            PathBuf::from("/tmp/memory-tool-test/memory.sqlite")
        );
    }

    #[test]
    fn memory_tool_data_dir_has_highest_priority() -> Result<()> {
        let root = data_root_from_env(
            Some(PathBuf::from("/custom/memory-tool")),
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/user")),
        )?;

        assert_eq!(root, PathBuf::from("/custom/memory-tool"));
        Ok(())
    }

    #[test]
    fn xdg_data_home_is_used_when_memory_tool_data_dir_is_missing() -> Result<()> {
        let root = data_root_from_env(
            None,
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/user")),
        )?;

        assert_eq!(root, PathBuf::from("/xdg/memory-tool"));
        Ok(())
    }

    #[test]
    fn home_fallback_is_used_when_overrides_are_missing() -> Result<()> {
        let root = data_root_from_env(None, None, Some(PathBuf::from("/home/user")))?;
        assert_eq!(root, PathBuf::from("/home/user/.local/share/memory-tool"));
        Ok(())
    }

    #[test]
    fn missing_home_returns_error() {
        let error = data_root_from_env(None, None, None).unwrap_err();

        assert!(error.to_string().contains("HOME is not set"));
    }

    #[test]
    fn ensure_data_dirs_creates_expected_directories() -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "memory-tool-test-{}-{}",
            std::process::id(),
            timestamp
        ));
        let dirs = DataDirs::from_root(&root);

        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }

        ensure_data_dirs(&dirs)?;
        ensure_data_dirs(&dirs)?;

        assert!(dirs.root.is_dir());
        assert!(dirs.inbox.is_dir());
        assert!(dirs.corpus.is_dir());
        assert!(!dirs.db.exists());

        std::fs::remove_dir_all(&root)?;
        Ok(())
    }
}

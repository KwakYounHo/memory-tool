use crate::store::paths::{data_dirs, ensure_data_dirs};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug)]
pub struct CliContext {
    pub db_path: PathBuf,
}

impl CliContext {
    pub fn load() -> Result<Self> {
        let dirs = data_dirs()?;
        ensure_data_dirs(&dirs)?;

        Ok(Self { db_path: dirs.db })
    }
}

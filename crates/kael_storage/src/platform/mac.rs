use std::path::PathBuf;

use crate::{Error, Result};

pub(crate) const BACKEND_NAME: &str = "sqlite+json-file";

pub(crate) fn base_config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or(Error::MissingEnvironmentVariable("HOME"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    base_config_dir()
}

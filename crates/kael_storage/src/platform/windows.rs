use std::path::PathBuf;

use crate::{Error, Result};

pub(crate) const BACKEND_NAME: &str = "sqlite+json-file";

pub(crate) fn base_config_dir() -> Result<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .ok_or(Error::MissingEnvironmentVariable("APPDATA or LOCALAPPDATA"))
}

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .ok_or(Error::MissingEnvironmentVariable("LOCALAPPDATA or APPDATA"))
}

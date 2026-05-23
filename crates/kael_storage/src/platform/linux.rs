use std::path::PathBuf;

use crate::{Error, Result};

pub(crate) const BACKEND_NAME: &str = "sqlite+json-file";

pub(crate) fn base_config_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or(Error::MissingEnvironmentVariable("XDG_CONFIG_HOME or HOME"))
}

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or(Error::MissingEnvironmentVariable("XDG_DATA_HOME or HOME"))
}

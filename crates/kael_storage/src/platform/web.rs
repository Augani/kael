use std::path::PathBuf;

use crate::{Error, Result};

pub(crate) const BACKEND_NAME: &str = "indexeddb+localstorage";

pub(crate) fn base_config_dir() -> Result<PathBuf> {
    Err(Error::BrowserPathUnsupported)
}

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    Err(Error::BrowserPathUnsupported)
}

//! Typed browser compatibility boundary for Kael's native SQLite API.
//!
//! Browser persistence is available through [`crate::PlatformKvStore`] and
//! [`crate::BlobStore`]. Shipping a silent SQL emulator here would change query semantics and
//! migration guarantees, so each SQLite entry point returns a typed unsupported error instead.

use std::{marker::PhantomData, path::Path};

use crate::{Error, Migration, Result};

/// Marker trait retained so shared generic code can name a database row type on every target.
pub trait FromRow: Sized {}

impl<T> FromRow for T {}

/// A browser-compatible marker for values supplied to an unavailable SQLite operation.
pub trait SqlParameter {}

impl<T: ?Sized> SqlParameter for T {}

/// An unavailable SQLite database handle on browser WebAssembly.
#[derive(Debug, Clone, Copy)]
pub struct Database;

/// An unavailable SQLite transaction on browser WebAssembly.
#[derive(Debug)]
pub struct Transaction<'connection> {
    marker: PhantomData<&'connection ()>,
}

impl Transaction<'_> {
    /// Reports that browser WebAssembly does not provide the native SQLite transaction backend.
    pub fn execute(&self, _sql: &str, _params: &[&dyn SqlParameter]) -> Result<usize> {
        unsupported("transaction.execute")
    }

    /// Reports that browser WebAssembly does not provide the native SQLite transaction backend.
    pub fn execute_batch(&self, _sql: &str) -> Result<()> {
        unsupported("transaction.execute_batch")
    }

    /// Reports that browser WebAssembly does not provide the native SQLite transaction backend.
    pub fn query<T: FromRow>(&self, _sql: &str, _params: &[&dyn SqlParameter]) -> Result<Vec<T>> {
        unsupported("transaction.query")
    }

    /// Reports that browser WebAssembly does not provide the native SQLite transaction backend.
    pub fn query_one<T: FromRow>(&self, _sql: &str, _params: &[&dyn SqlParameter]) -> Result<T> {
        unsupported("transaction.query_one")
    }

    /// Reports that browser WebAssembly does not provide the native SQLite transaction backend.
    pub fn rollback(self) -> Result<()> {
        unsupported("transaction.rollback")
    }
}

impl Database {
    /// Reports that browser WebAssembly cannot open a native file-backed SQLite database.
    pub async fn open(_path: impl AsRef<Path>) -> Result<Self> {
        unsupported("open")
    }

    /// Reports that browser WebAssembly cannot open a native file-backed SQLite database.
    pub async fn open_for_app(_app_id: &str, _name: &str) -> Result<Self> {
        unsupported("open_for_app")
    }

    /// Reports that browser WebAssembly does not bundle a SQLite engine.
    pub async fn open_in_memory() -> Result<Self> {
        unsupported("open_in_memory")
    }

    /// Returns no native path because browser storage is origin-scoped.
    pub const fn path(&self) -> Option<&Path> {
        None
    }

    /// Reports that SQL migrations are unavailable in the browser backend.
    pub async fn migrate(&self, _migrations: &[Migration]) -> Result<()> {
        unsupported("migrate")
    }

    /// Reports that SQL execution is unavailable in the browser backend.
    pub async fn execute(&self, _sql: &str, _params: &[&dyn SqlParameter]) -> Result<usize> {
        unsupported("execute")
    }

    /// Reports that SQL execution is unavailable in the browser backend.
    pub async fn execute_batch(&self, _sql: &str) -> Result<()> {
        unsupported("execute_batch")
    }

    /// Reports that SQL queries are unavailable in the browser backend.
    pub async fn query<T: FromRow + Send + 'static>(
        &self,
        _sql: &str,
        _params: &[&dyn SqlParameter],
    ) -> Result<Vec<T>> {
        unsupported("query")
    }

    /// Reports that SQL queries are unavailable in the browser backend.
    pub async fn query_one<T: FromRow + Send + 'static>(
        &self,
        _sql: &str,
        _params: &[&dyn SqlParameter],
    ) -> Result<T> {
        unsupported("query_one")
    }

    /// Reports that SQL transactions are unavailable in the browser backend.
    pub async fn transaction<F, R>(&self, _f: F) -> Result<R>
    where
        F: FnOnce(&Transaction<'_>) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        unsupported("transaction")
    }
}

fn unsupported<T>(operation: &'static str) -> Result<T> {
    Err(Error::BrowserSqlUnsupported { operation })
}

#[cfg(test)]
mod tests {
    use super::Database;
    use crate::Error;

    #[test]
    fn sqlite_boundary_is_typed() {
        let error = futures::executor::block_on(Database::open_in_memory()).unwrap_err();
        assert!(matches!(
            error,
            Error::BrowserSqlUnsupported {
                operation: "open_in_memory"
            }
        ));
    }
}

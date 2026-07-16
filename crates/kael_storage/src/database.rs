//! SQLite-backed database support.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use rusqlite::{
    Connection, OptionalExtension, Params, Row, ToSql, params_from_iter,
    types::{ToSqlOutput, Value},
};

use crate::{
    Error, Result,
    migration::{Migration, pending_migrations},
    platform,
};

#[cfg(test)]
use crate::migration::rollback_migrations;

const MIGRATIONS_TABLE: &str = "__kael_schema_migrations";

/// A trait for mapping query rows into application types.
pub trait FromRow: Sized {
    /// Builds a value from a SQLite row.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self>;
}

impl<T> FromRow for T
where
    T: rusqlite::types::FromSql,
{
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        row.get(0)
    }
}

/// A SQLite database handle.
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A transaction opened against a [`Database`].
pub struct Transaction<'connection> {
    inner: rusqlite::Transaction<'connection>,
}

impl<'connection> Transaction<'connection> {
    /// Executes a SQL statement within the transaction.
    pub fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize> {
        Ok(self.inner.execute(sql, params)?)
    }

    /// Executes a batch of SQL statements within the transaction.
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        Ok(self.inner.execute_batch(sql)?)
    }

    /// Runs a query and maps every row into `T`.
    pub fn query<T: FromRow>(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<T>> {
        query_with_connection(&self.inner, sql, params)
    }

    /// Runs a query and expects exactly one row.
    pub fn query_one<T: FromRow>(&self, sql: &str, params: &[&dyn ToSql]) -> Result<T> {
        query_one_with_connection(&self.inner, sql, params)
    }

    /// Rolls the transaction back.
    pub fn rollback(self) -> Result<()> {
        Ok(self.inner.rollback()?)
    }

    fn commit(self) -> Result<()> {
        Ok(self.inner.commit()?)
    }
}

impl Database {
    /// Opens or creates a SQLite database at the given path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        smol::unblock(move || {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|source| Error::io(parent.to_path_buf(), source))?;
            }

            let connection = Connection::open(&path)?;
            configure_connection(&connection)?;

            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
                path: Some(path),
            })
        })
        .await
    }

    /// Opens or creates a SQLite database using the platform storage layout for an app.
    pub async fn open_for_app(app_id: &str, name: &str) -> Result<Self> {
        let path = platform::database_path(app_id, name)?;
        Self::open(path).await
    }

    /// Opens an in-memory SQLite database.
    pub async fn open_in_memory() -> Result<Self> {
        smol::unblock(|| {
            let connection = Connection::open_in_memory()?;
            configure_connection(&connection)?;

            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
                path: None,
            })
        })
        .await
    }

    /// Returns the file path for this database, if it is persisted on disk.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Applies all pending migrations.
    pub async fn migrate(&self, migrations: &[Migration]) -> Result<()> {
        let connection = self.connection.clone();
        let migrations = migrations.to_vec();

        smol::unblock(move || {
            let mut connection = connection.lock();
            apply_migrations(&mut connection, &migrations)
        })
        .await
    }

    /// Executes a SQL statement.
    pub async fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize> {
        let connection = self.connection.clone();
        let sql = sql.to_string();
        let params = clone_params(params)?;

        smol::unblock(move || {
            let connection = connection.lock();
            Ok(connection.execute(&sql, params_from_iter(params.iter()))?)
        })
        .await
    }

    /// Executes a batch of SQL statements.
    pub async fn execute_batch(&self, sql: &str) -> Result<()> {
        let connection = self.connection.clone();
        let sql = sql.to_string();

        smol::unblock(move || {
            let connection = connection.lock();
            Ok(connection.execute_batch(&sql)?)
        })
        .await
    }

    /// Runs a query and maps every row into `T`.
    pub async fn query<T: FromRow + Send + 'static>(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
    ) -> Result<Vec<T>> {
        let connection = self.connection.clone();
        let sql = sql.to_string();
        let params = clone_params(params)?;

        smol::unblock(move || {
            let connection = connection.lock();
            query_with_connection(&connection, &sql, params_from_iter(params.iter()))
        })
        .await
    }

    /// Runs a query and expects exactly one row.
    pub async fn query_one<T: FromRow + Send + 'static>(
        &self,
        sql: &str,
        params: &[&dyn ToSql],
    ) -> Result<T> {
        let connection = self.connection.clone();
        let sql = sql.to_string();
        let params = clone_params(params)?;

        smol::unblock(move || {
            let connection = connection.lock();
            query_one_with_connection(&connection, &sql, params_from_iter(params.iter()))
        })
        .await
    }

    /// Runs the closure inside a SQLite transaction.
    pub async fn transaction<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Transaction<'_>) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let connection = self.connection.clone();

        smol::unblock(move || {
            let mut connection = connection.lock();
            let transaction = Transaction {
                inner: connection.transaction()?,
            };
            let output = f(&transaction)?;
            transaction.commit()?;
            Ok(output)
        })
        .await
    }
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;\nPRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

fn query_with_connection<T: FromRow, P: Params>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<T>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params, T::from_row)?;
    let mut results = Vec::new();

    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

fn query_one_with_connection<T: FromRow, P: Params>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<T> {
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query(params)?;

    let Some(row) = rows.next()? else {
        return Err(Error::RowNotFound);
    };
    let result = T::from_row(row)?;

    let mut row_count = 1usize;
    while rows.next()?.is_some() {
        row_count += 1;
    }

    if row_count == 1 {
        Ok(result)
    } else {
        Err(Error::UnexpectedRowCount(row_count))
    }
}

fn clone_params(params: &[&dyn ToSql]) -> Result<Vec<Value>> {
    params.iter().map(|param| clone_param(*param)).collect()
}

fn clone_param(param: &dyn ToSql) -> Result<Value> {
    #[allow(unreachable_patterns)]
    match param.to_sql()? {
        ToSqlOutput::Borrowed(value) => Ok(value.into()),
        ToSqlOutput::Owned(value) => Ok(value),
        _ => Err(
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(
                "unsupported SQLite parameter type",
            )))
            .into(),
        ),
    }
}

fn apply_migrations(connection: &mut Connection, migrations: &[Migration]) -> Result<()> {
    ensure_migrations_table(connection)?;
    let current_version = current_version(connection)?;
    let pending = pending_migrations(current_version, migrations)?;

    if pending.is_empty() {
        return Ok(());
    }

    let transaction = connection.transaction()?;

    for migration in pending {
        transaction.execute_batch(migration.up)?;
        transaction.execute(
            &format!("INSERT INTO {MIGRATIONS_TABLE} (version, description) VALUES (?1, ?2)"),
            [&migration.version as &dyn ToSql, &migration.description],
        )?;
    }

    transaction.commit()?;
    Ok(())
}

fn ensure_migrations_table(connection: &Connection) -> Result<()> {
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (\n    version INTEGER PRIMARY KEY,\n    description TEXT NOT NULL,\n    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP\n);"
    ))?;
    Ok(())
}

fn current_version(connection: &Connection) -> Result<u32> {
    let query = format!("SELECT MAX(version) FROM {MIGRATIONS_TABLE}");
    let version = connection
        .query_row(&query, [], |row| row.get::<_, Option<u32>>(0))
        .optional()?
        .flatten()
        .unwrap_or(0);
    Ok(version)
}

#[cfg(test)]
fn rollback_to(
    connection: &mut Connection,
    target_version: u32,
    migrations: &[Migration],
) -> Result<()> {
    ensure_migrations_table(connection)?;
    let current = current_version(connection)?;
    let rollback_plan = rollback_migrations(current, target_version, migrations)?;

    if rollback_plan.is_empty() {
        return Ok(());
    }

    let transaction = connection.transaction()?;

    for migration in rollback_plan {
        transaction.execute_batch(migration.down.expect("validated before use"))?;
        transaction.execute(
            &format!("DELETE FROM {MIGRATIONS_TABLE} WHERE version = ?1"),
            [&migration.version as &dyn ToSql],
        )?;
    }

    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Database, Migration, current_version, rollback_to};
    use crate::Error;

    #[derive(Debug, PartialEq, Eq)]
    struct Item {
        id: i64,
        name: String,
    }

    impl super::FromRow for Item {
        fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
            Ok(Self {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        }
    }

    const MIGRATIONS: [Migration; 2] = [
        Migration {
            version: 1,
            description: "create items",
            up: "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
            down: Some("DROP TABLE items;"),
        },
        Migration {
            version: 2,
            description: "add metadata",
            up: "ALTER TABLE items ADD COLUMN metadata TEXT;",
            down: Some(
                "CREATE TABLE items__rollback (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\nINSERT INTO items__rollback (id, name) SELECT id, name FROM items;\nDROP TABLE items;\nALTER TABLE items__rollback RENAME TO items;",
            ),
        },
    ];

    #[test]
    fn runs_migrations_and_queries_rows() {
        let database = futures::executor::block_on(Database::open_in_memory()).unwrap();
        futures::executor::block_on(database.migrate(&MIGRATIONS)).unwrap();
        futures::executor::block_on(database.execute(
            "INSERT INTO items (name, metadata) VALUES (?1, ?2)",
            &[&"widget", &"{}"],
        ))
        .unwrap();

        let rows = futures::executor::block_on(
            database.query::<Item>("SELECT id, name FROM items ORDER BY id", &[]),
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "widget");
    }

    #[test]
    fn rolls_transactions_back_on_error() {
        let database = futures::executor::block_on(Database::open_in_memory()).unwrap();
        futures::executor::block_on(
            database
                .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"),
        )
        .unwrap();

        let error = futures::executor::block_on(database.transaction(
            |transaction: &super::Transaction<'_>| -> crate::Result<()> {
                transaction.execute("INSERT INTO items (name) VALUES (?1)", &[&"widget"])?;
                Err(Error::UnexpectedRowCount(0))
            },
        ))
        .unwrap_err();

        assert!(matches!(error, Error::UnexpectedRowCount(0)));

        let count = futures::executor::block_on(
            database.query_one::<i64>("SELECT COUNT(*) FROM items", &[]),
        )
        .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn supports_rollback_for_test_coverage() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        super::configure_connection(&connection).unwrap();
        super::apply_migrations(&mut connection, &MIGRATIONS).unwrap();
        assert_eq!(current_version(&connection).unwrap(), 2);

        rollback_to(&mut connection, 1, &MIGRATIONS).unwrap();

        let pragma = connection
            .query_row("PRAGMA table_info(items)", [], |row| {
                row.get::<_, String>(1)
            })
            .unwrap();
        assert_eq!(pragma, "id");
        assert_eq!(current_version(&connection).unwrap(), 1);
    }

    #[test]
    fn query_one_requires_exactly_one_row() {
        let database = futures::executor::block_on(Database::open_in_memory()).unwrap();
        futures::executor::block_on(
            database
                .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"),
        )
        .unwrap();

        let error =
            futures::executor::block_on(database.query_one::<i64>("SELECT id FROM items", &[]))
                .unwrap_err();
        assert!(matches!(error, Error::RowNotFound));
    }

    #[test]
    fn query_one_reports_multiple_rows_without_materializing_values() {
        let database = futures::executor::block_on(Database::open_in_memory()).unwrap();
        futures::executor::block_on(
            database
                .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"),
        )
        .unwrap();
        futures::executor::block_on(database.execute(
            "INSERT INTO items (name) VALUES (?1), (?2)",
            &[&"first", &"second"],
        ))
        .unwrap();

        let error = futures::executor::block_on(
            database.query_one::<i64>("SELECT id FROM items ORDER BY id", &[]),
        )
        .unwrap_err();
        assert!(matches!(error, Error::UnexpectedRowCount(2)));
    }
}

//! Migration types and ordering helpers.

use crate::{Error, Result};

/// A database migration step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// The unique migration version.
    pub version: u32,
    /// A short human-readable description.
    pub description: &'static str,
    /// SQL executed when applying this migration.
    pub up: &'static str,
    /// Optional SQL executed when rolling back this migration.
    pub down: Option<&'static str>,
}

pub(crate) fn validate_migrations(migrations: &[Migration]) -> Result<()> {
    let mut previous_version = None;

    for migration in migrations {
        if let Some(previous_version) = previous_version {
            if migration.version == previous_version {
                return Err(Error::DuplicateMigrationVersion(migration.version));
            }

            if migration.version < previous_version {
                return Err(Error::InvalidMigrationOrder);
            }
        }

        previous_version = Some(migration.version);
    }

    Ok(())
}

pub(crate) fn pending_migrations(
    current_version: u32,
    migrations: &[Migration],
) -> Result<Vec<&Migration>> {
    validate_migrations(migrations)?;

    Ok(migrations
        .iter()
        .filter(|migration| migration.version > current_version)
        .collect())
}

#[cfg(test)]
pub(crate) fn rollback_migrations<'a>(
    current_version: u32,
    target_version: u32,
    migrations: &'a [Migration],
) -> Result<Vec<&'a Migration>> {
    validate_migrations(migrations)?;

    if target_version > current_version {
        return Err(Error::InvalidRollbackTarget {
            target: target_version,
            current: current_version,
        });
    }

    let mut plan = Vec::new();

    for migration in migrations.iter().rev() {
        if migration.version <= target_version {
            break;
        }

        if migration.version <= current_version {
            if migration.down.is_none() {
                return Err(Error::MissingDownMigration(migration.version));
            }

            plan.push(migration);
        }
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::{pending_migrations, rollback_migrations, validate_migrations, Migration};
    use crate::Error;

    const FIRST: Migration = Migration {
        version: 1,
        description: "create table",
        up: "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        down: Some("DROP TABLE items;"),
    };

    const SECOND: Migration = Migration {
        version: 2,
        description: "add metadata",
        up: "ALTER TABLE items ADD COLUMN metadata TEXT;",
        down: Some("ALTER TABLE items DROP COLUMN metadata;"),
    };

    #[test]
    fn rejects_duplicate_versions() {
        let error = validate_migrations(&[FIRST, FIRST]).unwrap_err();
        assert!(matches!(error, Error::DuplicateMigrationVersion(1)));
    }

    #[test]
    fn rejects_out_of_order_versions() {
        let error = validate_migrations(&[SECOND, FIRST]).unwrap_err();
        assert!(matches!(error, Error::InvalidMigrationOrder));
    }

    #[test]
    fn computes_pending_versions() {
        let pending = pending_migrations(1, &[FIRST, SECOND]).unwrap();
        assert_eq!(pending, vec![&SECOND]);
    }

    #[test]
    fn computes_rollback_plan_in_reverse_order() {
        let rollback = rollback_migrations(2, 0, &[FIRST, SECOND]).unwrap();
        assert_eq!(rollback, vec![&SECOND, &FIRST]);
    }
}

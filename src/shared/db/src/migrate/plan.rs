//! The comparison of the files a binary embeds with the versions a database has applied.

use sqlx::migrate::{AppliedMigration, Migration};

/// What a database lacks, has too much of, or has applied with other file content. Every list is sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub pending: Vec<i64>,
    pub unknown_newer: Vec<i64>,
    pub checksum_mismatch: Vec<i64>,
}

/// `known` are the `up` files of the binary, `applied` the rows of the bookkeeping table.
pub(crate) fn diff(known: &[&Migration], applied: &[AppliedMigration]) -> Plan {
    let mut plan = Plan::default();
    for migration in known {
        match applied.iter().find(|row| row.version == migration.version) {
            None => plan.pending.push(migration.version),
            Some(row) if row.checksum != migration.checksum => {
                plan.checksum_mismatch.push(migration.version);
            }
            Some(_) => {}
        }
    }
    for row in applied {
        if !known
            .iter()
            .any(|migration| migration.version == row.version)
        {
            plan.unknown_newer.push(row.version);
        }
    }
    plan.pending.sort_unstable();
    plan.unknown_newer.sort_unstable();
    plan.checksum_mismatch.sort_unstable();
    plan
}

#[cfg(test)]
mod tests {
    use sqlx::SqlSafeStr;
    use sqlx::migrate::MigrationType;

    use super::*;

    fn up(version: i64, sql: &'static str) -> Migration {
        Migration::new(
            version,
            "test".into(),
            MigrationType::ReversibleUp,
            sql.into_sql_str(),
            false,
        )
    }

    fn applied(migration: &Migration) -> AppliedMigration {
        AppliedMigration {
            version: migration.version,
            checksum: migration.checksum.clone(),
        }
    }

    #[test]
    fn equal_sets_have_nothing_to_report() {
        let (first, second) = (up(1, "SELECT 1"), up(2, "SELECT 2"));
        let rows = [applied(&first), applied(&second)];

        let plan = diff(&[&first, &second], &rows);

        assert_eq!(plan, Plan::default());
    }

    #[test]
    fn a_file_without_a_row_is_pending() {
        let (first, second, third) = (up(1, "SELECT 1"), up(2, "SELECT 2"), up(3, "SELECT 3"));
        let rows = [applied(&first)];

        let plan = diff(&[&first, &second, &third], &rows);

        assert_eq!(plan.pending, [2, 3]);
        assert!(plan.unknown_newer.is_empty() && plan.checksum_mismatch.is_empty());
    }

    #[test]
    fn a_row_without_a_file_is_unknown_newer() {
        let (first, second) = (up(1, "SELECT 1"), up(2, "SELECT 2"));
        let rows = [applied(&first), applied(&second)];

        let plan = diff(&[&first], &rows);

        assert_eq!(plan.unknown_newer, [2]);
        assert!(plan.pending.is_empty());
    }

    #[test]
    fn a_row_with_other_content_is_a_checksum_mismatch() {
        let original = up(1, "SELECT 1");
        let edited = up(1, "SELECT 1 -- edited");
        let rows = [applied(&original)];

        let plan = diff(&[&edited], &rows);

        assert_eq!(plan.checksum_mismatch, [1]);
        assert!(plan.pending.is_empty() && plan.unknown_newer.is_empty());
    }

    #[test]
    fn lists_are_sorted_whatever_the_input_order() {
        let (one, two, three) = (up(1, "a"), up(2, "b"), up(3, "c"));
        let rows = [applied(&three), applied(&two)];

        let plan = diff(&[&three, &two, &one], &rows);

        assert_eq!(plan.pending, [1]);
    }
}

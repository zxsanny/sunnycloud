//! The database check of `doctor` (E-22/c14b): reachability, server version, the API role's privileges, the
//! schema state and the clock skew against the database server, from one connection that does not refuse a
//! wrongly privileged role, so the finding names the fault instead of the whole `doctor` failing to start.

use std::sync::Arc;
use std::time::Duration;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};
use sc_clock::Clock;
use sc_db::{Db, DbConfig, DbInspection, MigrationError, SchemaState};
use sc_observability::log_error;

const REQUIRED_MAJOR: &str = "18";
const MAX_SKEW: Duration = Duration::from_secs(2);
const ROLE_REMEDY: &str = "connect as the plain `sunnycloud` role the install script creates: not a superuser, no BYPASSRLS, not the schema owner";

pub struct DatabaseCheck {
    config: DbConfig,
    clock: Arc<dyn Clock>,
}

impl DatabaseCheck {
    pub fn new(config: DbConfig, clock: Arc<dyn Clock>) -> Self {
        DatabaseCheck { config, clock }
    }
}

fn finding(level: FindingLevel, message: impl Into<String>, remedy: &str) -> Finding {
    Finding {
        level,
        message: message.into(),
        remedy: remedy.to_owned(),
    }
}

fn ok(message: &str) -> Finding {
    finding(FindingLevel::Ok, message, "")
}

fn fail(message: impl Into<String>, remedy: &str) -> Finding {
    finding(FindingLevel::Fail, message, remedy)
}

fn version(seen: &str) -> Finding {
    let major = seen.split('.').next().unwrap_or(seen);
    if major == REQUIRED_MAJOR {
        ok(&format!("PostgreSQL {seen}"))
    } else {
        fail(
            format!(
                "the database server is PostgreSQL {seen}, version {REQUIRED_MAJOR} is required"
            ),
            "run PostgreSQL 18",
        )
    }
}

fn role(problem: Option<&str>) -> Finding {
    match problem {
        None => ok("the database role is the plain API role"),
        Some(problem) => fail(problem, ROLE_REMEDY),
    }
}

fn schema(state: &Result<SchemaState, MigrationError>) -> Finding {
    const MIGRATE: &str = "run `sunnycloud migrate`";
    match state {
        Ok(SchemaState::Current) => ok("the schema is current"),
        Ok(SchemaState::Pending { n }) => {
            fail(format!("{n} migration(s) are not applied"), MIGRATE)
        }
        Ok(SchemaState::UnknownNewer) => fail(
            "the database has migrations this binary does not know",
            "run the release that matches the database, or restore the older backup",
        ),
        Err(MigrationError::Dirty { version }) => fail(
            format!("migration {version} is half applied"),
            "repair or restore the database, then run `sunnycloud migrate`",
        ),
        Err(error) => {
            log_error!("schema check failed", err = error);
            fail(
                "the schema state could not be read",
                "run `sunnycloud migrate status` for the reason",
            )
        }
    }
}

fn skew(server_micros: i64, host_micros: i64) -> Finding {
    let apart = Duration::from_micros(server_micros.abs_diff(host_micros));
    if apart > MAX_SKEW {
        finding(
            FindingLevel::Warn,
            format!(
                "the database clock differs from this host by {} s",
                apart.as_secs()
            ),
            "synchronise both clocks (NTP)",
        )
    } else {
        ok("the database clock agrees with this host")
    }
}

fn findings(seen: &DbInspection, host_micros: i64) -> Vec<Finding> {
    vec![
        version(&seen.server_version),
        role(seen.role_problem),
        schema(&seen.schema),
        skew(seen.server_clock_micros, host_micros),
    ]
}

#[async_trait]
impl DoctorCheck for DatabaseCheck {
    fn name(&self) -> &'static str {
        "database"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[]
    }

    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        match Db::inspect(&self.config).await {
            Ok(seen) => findings(&seen, self.clock.now_utc().as_unix_micros()),
            Err(error) => {
                log_error!("database inspection failed", err = &error);
                vec![fail(
                    "the database cannot be reached or read",
                    "start the database and check the `db_api_url` secret",
                )]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 1_000_000;

    #[test]
    fn only_server_version_18_passes() {
        // Arrange
        let cases = [
            ("18.1", FindingLevel::Ok),
            ("17.4", FindingLevel::Fail),
            ("19beta1", FindingLevel::Fail),
        ];

        for (seen, level) in cases {
            // Act
            let result = version(seen);

            // Assert
            assert_eq!(result.level, level, "{seen}");
        }
    }

    #[test]
    fn a_skew_above_two_seconds_is_a_warning_and_one_second_is_fine_in_both_directions() {
        // Arrange
        let host = 1_000 * SECOND;

        // Act
        let levels = [
            skew(host + 3 * SECOND, host).level,
            skew(host - 3 * SECOND, host).level,
            skew(host + SECOND, host).level,
            skew(host + 2 * SECOND, host).level,
        ];

        // Assert
        assert_eq!(
            levels,
            [
                FindingLevel::Warn,
                FindingLevel::Warn,
                FindingLevel::Ok,
                FindingLevel::Ok
            ]
        );
    }

    #[test]
    fn every_schema_fault_is_a_fail_naming_the_remedy_and_current_is_ok() {
        // Arrange
        let faults: [Result<SchemaState, MigrationError>; 4] = [
            Ok(SchemaState::Pending { n: 2 }),
            Ok(SchemaState::UnknownNewer),
            Err(MigrationError::Dirty { version: 7 }),
            Err(MigrationError::ChecksumMismatch { version: 7 }),
        ];

        // Act
        let current = schema(&Ok(SchemaState::Current));
        let found: Vec<Finding> = faults.iter().map(schema).collect();

        // Assert
        assert_eq!(current.level, FindingLevel::Ok);
        assert!(
            found
                .iter()
                .all(|item| item.level == FindingLevel::Fail && !item.remedy.is_empty())
        );
        assert!(found[0].remedy.contains("sunnycloud migrate"));
    }

    #[test]
    fn a_role_problem_is_a_fail_with_the_problem_as_message() {
        // Arrange
        let problem = Some("the API role has BYPASSRLS");

        // Act
        let found = role(problem);
        let healthy = role(None);

        // Assert
        assert_eq!(
            (found.level, found.message.as_str()),
            (FindingLevel::Fail, "the API role has BYPASSRLS")
        );
        assert_eq!(healthy.level, FindingLevel::Ok);
    }
}

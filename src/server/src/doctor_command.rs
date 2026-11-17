//! `doctor` (E-22/c14a): runs every registered `DoctorCheck`, or a tagged subset, and reports each finding.
//!
//! This is aggregation only. The checks are their owners', registered in `root::DoctorChecks`; other commands
//! (`admin epoch-bump` runs the `restore` subset) call [`run_checks`] and render the findings themselves.

use std::sync::Arc;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel};
use serde_json::{Map, Value};

use crate::commands::DoctorArgs;
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::outcome::{Class, Outcome};
use crate::root::{DoctorChecks, PreFlightChecks};

/// Which of the registered checks run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckSelection {
    All,
    Tagged(DoctorTag),
}

/// One finding with the name of the check that returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckFinding {
    pub check: &'static str,
    pub finding: Finding,
}

/// Runs the selected checks one after the other in registration order and returns their findings in that order.
pub async fn run_checks(
    checks: &[Arc<dyn DoctorCheck>],
    selection: CheckSelection,
    ctx: &DoctorContext,
) -> Vec<CheckFinding> {
    let mut results = Vec::new();
    for check in checks.iter().filter(|check| selected(check, selection)) {
        for finding in check.run(ctx).await {
            results.push(CheckFinding {
                check: check.name(),
                finding,
            });
        }
    }
    results
}

fn selected(check: &Arc<dyn DoctorCheck>, selection: CheckSelection) -> bool {
    match selection {
        CheckSelection::All => true,
        CheckSelection::Tagged(tag) => check.tags().contains(&tag),
    }
}

/// `doctor`: `--pre-flight` runs only the checks tagged `pre-flight`; no other option narrows the set.
pub(crate) async fn run(args: &DoctorArgs, services: &ServiceContext) -> Result<Outcome, CliError> {
    let checks = if args.pre_flight {
        services.get::<PreFlightChecks>().await?.0.clone()
    } else {
        services.get::<DoctorChecks>().await?.0.clone()
    };
    let ctx = DoctorContext {
        pre_flight: args.pre_flight,
        backup_interval: args.backup_interval,
        attest_backup: args.attest_backup,
    };
    let selection = if args.pre_flight {
        CheckSelection::Tagged(DoctorTag::PreFlight)
    } else {
        CheckSelection::All
    };
    Ok(report(&run_checks(&checks, selection, &ctx).await))
}

/// The outcome of a run: Fail anywhere is exit 2, Warn anywhere (and no Fail) exit 1, otherwise 0.
pub fn report(findings: &[CheckFinding]) -> Outcome {
    let worst = findings
        .iter()
        .map(|item| item.finding.level)
        .fold(FindingLevel::Ok, worse);
    let class = match worst {
        FindingLevel::Ok => Class::Success,
        FindingLevel::Warn => Class::Warnings,
        FindingLevel::Fail => Class::Findings,
    };
    Outcome::new(class).field("findings", findings_value(findings))
}

/// The findings as the list both writers print.
pub fn findings_value(findings: &[CheckFinding]) -> Value {
    Value::Array(findings.iter().map(item).collect())
}

fn worse(left: FindingLevel, right: FindingLevel) -> FindingLevel {
    match (left, right) {
        (FindingLevel::Fail, _) | (_, FindingLevel::Fail) => FindingLevel::Fail,
        (FindingLevel::Warn, _) | (_, FindingLevel::Warn) => FindingLevel::Warn,
        _ => FindingLevel::Ok,
    }
}

fn item(finding: &CheckFinding) -> Value {
    let mut object = Map::new();
    object.insert("check".to_owned(), Value::from(finding.check));
    object.insert(
        "level".to_owned(),
        Value::from(level_name(finding.finding.level)),
    );
    object.insert(
        "message".to_owned(),
        Value::from(finding.finding.message.as_str()),
    );
    object.insert(
        "remedy".to_owned(),
        Value::from(finding.finding.remedy.as_str()),
    );
    Value::Object(object)
}

const fn level_name(level: FindingLevel) -> &'static str {
    match level {
        FindingLevel::Ok => "ok",
        FindingLevel::Warn => "warn",
        FindingLevel::Fail => "fail",
    }
}

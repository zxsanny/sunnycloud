//! AC-7: the coverage map assigns every expected result of IT-12-1..19 to exactly one place.

use std::collections::BTreeSet;

use crate::common::{crate_dir, read};

const PLACES: [&str; 5] = ["suite", "command AZ-", "pipeline AZ-", "owner ", "E-31"];
const STATUSES: [&str; 4] = ["pass", "known failure", "blocked", "elsewhere"];

struct Row {
    case: String,
    part: String,
    place: String,
    status: String,
    proof: String,
}

fn map_rows(map: &str) -> Vec<Row> {
    map.lines()
        .filter(|line| line.starts_with("| IT-12-"))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect();
            assert_eq!(cells.len(), 5, "a map row has five cells: {line}");
            Row {
                case: cells[0].clone(),
                part: cells[1].clone(),
                place: cells[2].clone(),
                status: cells[3].clone(),
                proof: cells[4].clone(),
            }
        })
        .collect()
}

/// The test a proof names as `` `path::name` ``: its file text and the attributes just above it.
fn named_test(proof: &str) -> Option<(String, String, bool)> {
    let token = proof
        .split('`')
        .nth(1)
        .filter(|token| token.contains("::"))?;
    let (path, name) = token.split_once("::")?;
    let text = read(&crate_dir().join(path));
    let at = text.find(&format!("fn {name}("))?;
    let above = text[..at].rsplit("\n\n").next().unwrap_or("");
    Some((path.to_owned(), name.to_owned(), above.contains("#[ignore")))
}

#[test]
fn every_case_of_the_spec_has_rows_each_part_has_one_place_and_each_named_test_exists() {
    // Arrange
    let spec =
        read(&crate_dir().join("../../_docs/02_document/components/12_operator_cli/tests.md"));
    let cases: Vec<String> = (1..=19).map(|number| format!("IT-12-{number}")).collect();
    let rows = map_rows(&read(&crate_dir().join("tests/it12_coverage_map.md")));

    // Act
    let unknown: Vec<&str> = rows
        .iter()
        .map(|row| row.case.as_str())
        .filter(|case| !cases.iter().any(|c| c == case))
        .collect();
    let uncovered: Vec<&String> = cases
        .iter()
        .filter(|case| !rows.iter().any(|row| &row.case == *case))
        .collect();
    let missing_heading: Vec<&String> = cases
        .iter()
        .filter(|case| !spec.contains(&format!("### {case}:")))
        .collect();
    let mut seen = BTreeSet::new();
    let twice: Vec<String> = rows
        .iter()
        .filter(|row| !seen.insert((row.case.clone(), row.part.clone())))
        .map(|row| format!("{}: {}", row.case, row.part))
        .collect();
    let bad_place: Vec<String> = rows
        .iter()
        .filter(|row| !PLACES.iter().any(|place| row.place.starts_with(place)))
        .map(|row| format!("{}: {}", row.case, row.place))
        .collect();
    let bad_status: Vec<String> = rows
        .iter()
        .filter(|row| !STATUSES.contains(&row.status.as_str()))
        .map(|row| format!("{}: {}", row.case, row.status))
        .collect();
    let mut wrong_proof = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.status == "pass" || row.status == "known failure")
    {
        match named_test(&row.proof) {
            None => wrong_proof.push(format!(
                "{} / {}: no existing test named",
                row.case, row.part
            )),
            Some((path, name, ignored)) => {
                if ignored != (row.status == "known failure") {
                    wrong_proof.push(format!(
                        "{path}::{name}: ignored is {ignored}, status is {}",
                        row.status
                    ));
                }
            }
        }
    }

    // Assert
    assert!(
        missing_heading.is_empty(),
        "the spec lacks {missing_heading:?}"
    );
    assert!(
        unknown.is_empty(),
        "rows for cases the spec lacks: {unknown:?}"
    );
    assert!(uncovered.is_empty(), "no row for {uncovered:?}");
    assert!(twice.is_empty(), "a part is assigned twice: {twice:?}");
    assert!(bad_place.is_empty(), "not an allowed place: {bad_place:?}");
    assert!(
        bad_status.is_empty(),
        "not an allowed status: {bad_status:?}"
    );
    assert!(wrong_proof.is_empty(), "{wrong_proof:?}");
}

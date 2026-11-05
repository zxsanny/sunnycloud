mod common;

/// NFR: `validate_name` plus `name_key` under 5 microseconds for a typical name. Timing is only
/// meaningful with optimisations, so the check runs in release test builds.
#[cfg(not(debug_assertions))]
#[test]
fn validate_and_key_take_under_five_microseconds() {
    use std::hint::black_box;
    use std::time::Instant;

    let names = [
        "Quarterly report 2026.xlsx",
        "IMG_20261105_143059.jpg",
        "caf\u{e9} menu.pdf",
        "README.MD",
    ];
    let rounds = 50_000u32;
    let started = Instant::now();
    for i in 0..rounds {
        let name = common::valid(black_box(names[i as usize % names.len()]));
        black_box(sc_names::name_key(&name));
    }
    let per_call = started.elapsed() / rounds;
    assert!(per_call.as_micros() < 5, "{per_call:?} per validate+key");
}

#[cfg(debug_assertions)]
#[test]
fn performance_budget_is_checked_in_release_builds() {
    assert!(common::valid("typical name.txt").as_str().len() < 255);
}

//! Pearson's chi-square goodness-of-fit test of byte values against the uniform distribution (ST-06-2). The
//! critical value is the Wilson–Hilferty approximation of the chi-square quantile, which is within 0.1 of the
//! table value at 255 degrees of freedom.

/// The standard normal quantile at 1 - 0.001: uniformity is rejected at p = 0.001.
const Z_AT_P_0_001: f64 = 3.090_232_306;
const BINS: usize = 256;

/// The chi-square statistic of `bytes` against 256 equally likely values.
pub fn statistic(bytes: &[u8]) -> f64 {
    let mut counts = [0u64; BINS];
    for byte in bytes {
        counts[usize::from(*byte)] += 1;
    }
    let expected = bytes.len() as f64 / BINS as f64;
    counts
        .iter()
        .map(|count| {
            let delta = *count as f64 - expected;
            delta * delta / expected
        })
        .sum()
}

/// The chi-square value that `degrees` degrees of freedom exceed with probability 0.001.
pub fn critical_value(degrees: f64) -> f64 {
    let term = 2.0 / (9.0 * degrees);
    degrees * (1.0 - term + Z_AT_P_0_001 * term.sqrt()).powi(3)
}

/// Whether the test does not reject uniformity of `bytes` at p = 0.001.
pub fn uniform_not_rejected(bytes: &[u8]) -> bool {
    statistic(bytes) <= critical_value((BINS - 1) as f64)
}

/// 320,000 bytes of SHA-256 in counter mode: a fixed sample that is uniform for this test.
fn counter_stream() -> Vec<u8> {
    (0u32..10_000)
        .flat_map(|n| *sc_ids::sha256_token(n.to_string()).as_bytes())
        .collect()
}

#[test]
fn critical_value_at_255_degrees_matches_the_table() {
    // Arrange
    let table = 330.520;

    // Act
    let approximated = critical_value(255.0);

    // Assert
    assert!((approximated - table).abs() < 0.1, "{approximated}");
}

#[test]
fn a_known_uniform_sample_is_accepted() {
    // Arrange
    let flat: Vec<u8> = (0..1250).flat_map(|_| 0..=255u8).collect();
    let stream = counter_stream();

    // Act
    let accepted = (uniform_not_rejected(&flat), uniform_not_rejected(&stream));

    // Assert
    assert_eq!(accepted, (true, true), "{}", statistic(&stream));
}

#[test]
fn a_biased_sample_is_rejected() {
    // Arrange
    let mut zero_heavy = counter_stream();
    for byte in zero_heavy.iter_mut().step_by(50) {
        *byte = 0;
    }
    let text = "sc_shr_plaintext tokens are not random "
        .repeat(8_000)
        .into_bytes();

    // Act
    let accepted = (
        uniform_not_rejected(&zero_heavy),
        uniform_not_rejected(&text),
    );

    // Assert
    assert_eq!(accepted, (false, false), "{}", statistic(&zero_heavy));
}

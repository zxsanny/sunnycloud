//! The read-only test data of the 06 security and performance suites (06 `tests.md`, Test Data Management): a
//! seeded generator, the password corpus (ASCII, Unicode and 1 kB passwords, a static file) and the token corpus
//! (random tokens and near misses of live tokens: one character changed, truncated, case changed).

use sc_ids::RawToken;
use sunnycloud_sharing::SHARE_TOKEN_PREFIX;

use crate::visitor_support::presented;

/// One password per line; no line has leading or trailing blanks.
const PASSWORD_CORPUS: &str = include_str!("password_corpus.txt");

/// The characters of a token body (URL-safe base64).
const BODY_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// The length of a token body: 32 bytes as unpadded base64.
pub const BODY_LEN: usize = 43;

/// xorshift64*: deterministic for a seed, so a corpus is the same on every run.
pub struct Seeded(u64);

impl Seeded {
    pub fn new(seed: u64) -> Seeded {
        Seeded(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bound fits u64");
        usize::try_from(self.next_u64() % bound).expect("below a usize bound")
    }
}

/// Every password of the corpus.
pub fn passwords() -> Vec<&'static str> {
    PASSWORD_CORPUS.lines().collect()
}

/// The forms a password or token takes in a captured text: as is, JSON-escaped (a `row_to_json` dump) and
/// Debug-escaped (a log field).
pub fn plaintext_forms(secret: &str) -> [String; 3] {
    let json = secret.replace('\\', "\\\\").replace('"', "\\\"");
    let debug = format!("{secret:?}");
    let debug = debug[1..debug.len() - 1].to_owned();
    [secret.to_owned(), json, debug]
}

/// How a corpus token was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guess {
    Random,
    OneCharChanged,
    Truncated,
    CaseChanged,
}

/// `count` tokens, the four kinds interleaved; near misses come from `live` (token texts) and none equals one.
pub fn token_corpus(live: &[String], count: usize, seed: u64) -> Vec<(Guess, RawToken)> {
    let mut rng = Seeded::new(seed);
    let kinds = [
        Guess::Random,
        Guess::OneCharChanged,
        Guess::Truncated,
        Guess::CaseChanged,
    ];
    let mut corpus = Vec::with_capacity(count);
    while corpus.len() < count {
        let kind = kinds[corpus.len() % kinds.len()];
        let source = &live[rng.below(live.len())];
        let text = match kind {
            Guess::Random => random_token(&mut rng),
            Guess::OneCharChanged => one_char_changed(source, &mut rng),
            Guess::Truncated => source[..SHARE_TOKEN_PREFIX.len() + rng.below(BODY_LEN)].to_owned(),
            Guess::CaseChanged => case_changed(source, &mut rng),
        };
        if !live.contains(&text) {
            corpus.push((kind, presented(text)));
        }
    }
    corpus
}

fn random_token(rng: &mut Seeded) -> String {
    let body: String = (0..BODY_LEN)
        .map(|_| char::from(BODY_ALPHABET[rng.below(BODY_ALPHABET.len())]))
        .collect();
    format!("{SHARE_TOKEN_PREFIX}{body}")
}

fn one_char_changed(source: &str, rng: &mut Seeded) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let at = SHARE_TOKEN_PREFIX.len() + rng.below(BODY_LEN);
    let mut replacement = bytes[at];
    while replacement == bytes[at] {
        replacement = BODY_ALPHABET[rng.below(BODY_ALPHABET.len())];
    }
    bytes[at] = replacement;
    String::from_utf8(bytes).expect("ASCII token")
}

/// Every second case-changed guess upper-cases the whole token, the others flip the case of one body letter.
fn case_changed(source: &str, rng: &mut Seeded) -> String {
    if rng.below(2) == 0 {
        return source.to_ascii_uppercase();
    }
    let letters: Vec<usize> = source
        .bytes()
        .enumerate()
        .skip(SHARE_TOKEN_PREFIX.len())
        .filter(|(_, byte)| byte.is_ascii_alphabetic())
        .map(|(at, _)| at)
        .collect();
    let mut bytes = source.as_bytes().to_vec();
    let at = letters[rng.below(letters.len())];
    bytes[at] = if bytes[at].is_ascii_lowercase() {
        bytes[at].to_ascii_uppercase()
    } else {
        bytes[at].to_ascii_lowercase()
    };
    String::from_utf8(bytes).expect("ASCII token")
}

#[test]
fn the_password_corpus_has_ascii_unicode_and_kilobyte_passwords() {
    // Arrange
    let corpus = passwords();

    // Act
    let ascii = corpus.iter().filter(|p| p.is_ascii()).count();
    let kilobyte = corpus.iter().filter(|p| p.len() >= 1000).count();
    let total: usize = corpus.iter().map(|p| p.len()).sum();

    // Assert
    assert!(ascii >= 5 && corpus.len() - ascii >= 5, "{corpus:?}");
    assert!(kilobyte >= 3);
    assert!(total < 50 * 1024);
    assert!(corpus.iter().all(|p| p.len() >= 10 && p.trim() == *p));
}

#[test]
fn the_token_corpus_is_seeded_interleaved_and_never_hits_a_live_token() {
    // Arrange
    let live: Vec<String> = (0..3).map(|n| random_token(&mut Seeded::new(n))).collect();

    // Act
    let first = token_corpus(&live, 400, 7);
    let again = token_corpus(&live, 400, 7);

    // Assert
    let text = |corpus: &[(Guess, RawToken)]| -> Vec<String> {
        corpus.iter().map(|(_, t)| t.expose().to_owned()).collect()
    };
    assert_eq!(text(&first), text(&again));
    assert!(text(&first).iter().all(|t| !live.contains(t)));
    assert!(first.iter().all(|(kind, token)| match kind {
        Guess::Truncated => token.expose().len() < SHARE_TOKEN_PREFIX.len() + BODY_LEN,
        _ => token.expose().len() == SHARE_TOKEN_PREFIX.len() + BODY_LEN,
    }));
    assert_eq!(first[1].0, Guess::OneCharChanged);
    assert!(text(&first).iter().map(String::len).sum::<usize>() < 2 * 1024 * 1024);
}

mod common;

use std::time::Instant;

use common::*;
use sc_config::{ApiSettings, ConfigError, ConfigSources, Reader, TABLE, load};

/// xorshift64: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let index = usize::try_from(self.next() % items.len() as u64).expect("index");
        &items[index]
    }
}

const VALUES: &[&str] = &[
    "",
    "0",
    "1",
    "-1",
    "+1",
    "007",
    "1.5",
    "true",
    "off",
    "on",
    "a,b",
    ",",
    "5s",
    "5x",
    "7d",
    "90d",
    "91d",
    "99999999999999999999999d",
    "18446744073709551615d",
    "18446744073709551615TiB",
    "4MiB",
    "0.0.0.0:80",
    ":9090",
    "https://x.example",
    "http://x",
    "\u{1f4a5}",
    "a\0b",
    "  ",
    "{}",
    "[1]",
    "production",
    "test",
    "development",
    "staging",
    "/etc/passwd",
    "../..",
];

fn random_name(rng: &mut Rng) -> String {
    let key = rng.pick(TABLE).key;
    match rng.next() % 6 {
        0 => format!("SUNNYCLOUD_{}", key.to_uppercase()),
        1 => format!("SUNNYCLOUD_{}_FILE", key.to_uppercase()),
        2 => format!("SUNNYCLOUD_{key}"),
        3 => format!("SUNNYCLOUD_X{}", rng.next() % 50),
        4 => key.to_uppercase(),
        _ => format!("SUNNYCLOUD_{}", key.to_uppercase()),
    }
}

#[test]
fn load_never_panics_on_random_environment_maps() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3_000 {
        let mut sources = ConfigSources::default();
        for _ in 0..(rng.next() % 12) {
            let name = random_name(&mut rng);
            sources.env.insert(name, (*rng.pick(VALUES)).to_owned());
        }
        for reader in READERS {
            match load_reader(reader, &sources) {
                Ok(()) => {}
                Err(ConfigError::Invalid(problems)) => assert!(!problems.is_empty()),
                Err(ConfigError::MissingSecret(key)) => assert!(!key.is_empty()),
                Err(ConfigError::Unreadable(..)) => {}
            }
        }
    }
}

#[test]
fn random_overrides_on_a_valid_base_either_load_or_name_a_key() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..1_000 {
        let mut sources = valid_sources(Reader::Api);
        for _ in 0..(1 + rng.next() % 6) {
            let spec = rng.pick(TABLE);
            if spec.secret {
                continue;
            }
            set_key(&mut sources, spec.key, rng.pick(VALUES));
        }
        match load::<ApiSettings>(&sources) {
            Ok(_) => {}
            Err(ConfigError::Invalid(problems)) => {
                for p in &problems {
                    assert!(
                        TABLE.iter().any(|s| s.key == p.key) || p.key.starts_with("SUNNYCLOUD_"),
                        "{p:?}"
                    );
                }
            }
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn loading_the_full_table_takes_under_50_ms() {
    let sources = valid_sources(Reader::Api);
    let started = Instant::now();
    let loaded = load::<ApiSettings>(&sources);
    let elapsed = started.elapsed();

    assert!(loaded.is_ok(), "{:?}", loaded.err());
    assert!(elapsed.as_millis() < 50, "load took {elapsed:?}");
}

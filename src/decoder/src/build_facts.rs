//! The library versions and build time of the running image, from the facts file the image build
//! writes (`deploy/docker/media-metadata.sh`). One source: the image check proves the file equals
//! the pinned versions, and the container suite compares it with the tools' own `--version`.

use std::fs;
use std::io;
use std::path::Path;

use media_shared::decoder_protocol::Capabilities;
use sc_observability::log_error;
use serde_json::Value;
use thiserror::Error;

/// Where the image build writes the facts.
pub const BUILD_FACTS_PATH: &str = "/usr/share/sunnycloud/build-facts.json";

const UNREPORTED_VERSION: &str = "unknown";
/// The lowest timestamp the protocol accepts; it reads as "built long ago" in the operator's image age.
const UNREPORTED_TIMESTAMP: u64 = 1;
const MAX_TEXT_BYTES: usize = 128;
const MAX_TIMESTAMP: u64 = 4_102_444_800;

#[derive(Debug, Error)]
enum FactsError {
    #[error("cannot read the build facts file")]
    Read(#[from] io::Error),
    #[error("the build facts are not valid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFacts {
    pub vips: String,
    pub ffmpeg: String,
    pub libheif: String,
    pub build_timestamp: u64,
}

impl BuildFacts {
    /// The facts of the file at `path`. A file that is missing or does not hold valid facts (a runner
    /// started outside the image) is logged and answered as "not reported", never as a made-up value.
    pub fn load(path: &Path) -> Self {
        match read(path) {
            Ok(facts) => facts,
            Err(error) => {
                log_error!("build facts are not reported", err = &error);
                Self::unreported()
            }
        }
    }

    pub fn unreported() -> Self {
        Self {
            vips: UNREPORTED_VERSION.into(),
            ffmpeg: UNREPORTED_VERSION.into(),
            libheif: UNREPORTED_VERSION.into(),
            build_timestamp: UNREPORTED_TIMESTAMP,
        }
    }

    /// The `hello` answer; `nvenc` stays off until the probe exists (E-19/c13).
    pub fn capabilities(&self, protocol: u32) -> Capabilities {
        Capabilities {
            protocol,
            vips: self.vips.clone(),
            ffmpeg: self.ffmpeg.clone(),
            libheif: self.libheif.clone(),
            build_timestamp: self.build_timestamp,
            nvenc_probe_ok: false,
            nvenc_sessions: 0,
        }
    }
}

fn read(path: &Path) -> Result<BuildFacts, FactsError> {
    parse(&fs::read_to_string(path)?)
}

fn parse(text: &str) -> Result<BuildFacts, FactsError> {
    let facts: Value =
        serde_json::from_str(text).map_err(|error| FactsError::Invalid(error.to_string()))?;
    let version = |key: &str| {
        let value = facts.get(key).and_then(Value::as_str).unwrap_or_default();
        let printable = value.bytes().all(|byte| byte.is_ascii_graphic());
        if value.is_empty() || value.len() > MAX_TEXT_BYTES || !printable {
            return Err(FactsError::Invalid(format!(
                "{key} is not 1..=128 printable ASCII bytes"
            )));
        }
        Ok(value.to_owned())
    };
    let build_timestamp = facts
        .get("build_timestamp")
        .and_then(Value::as_u64)
        .filter(|stamp| (1..=MAX_TIMESTAMP).contains(stamp))
        .ok_or_else(|| FactsError::Invalid("build_timestamp is not Unix seconds".into()))?;
    Ok(BuildFacts {
        vips: version("libvips")?,
        ffmpeg: version("ffmpeg")?,
        libheif: version("libheif")?,
        build_timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FACTS: &str = r#"{"libheif": "1.23.7", "libvips": "8.18.7", "ffmpeg": "9.1", "build_timestamp": 1790000000}"#;

    #[test]
    fn the_facts_the_image_build_writes_are_read() {
        let facts = parse(FACTS).expect("valid facts");

        assert_eq!(
            facts,
            BuildFacts {
                vips: "8.18.7".into(),
                ffmpeg: "9.1".into(),
                libheif: "1.23.7".into(),
                build_timestamp: 1_790_000_000,
            }
        );
    }

    #[test]
    fn the_capabilities_carry_the_facts_and_the_nvenc_fields_off() {
        let capabilities = parse(FACTS).expect("valid facts").capabilities(2);

        assert_eq!(
            (
                capabilities.protocol,
                capabilities.vips.as_str(),
                capabilities.ffmpeg.as_str(),
                capabilities.libheif.as_str(),
                capabilities.build_timestamp,
                capabilities.nvenc_probe_ok,
                capabilities.nvenc_sessions,
            ),
            (2, "8.18.7", "9.1", "1.23.7", 1_790_000_000, false, 0)
        );
    }

    #[test]
    fn a_missing_empty_spaced_or_misdated_fact_is_refused() {
        let bad = [
            FACTS.replace("8.18.7", ""),
            FACTS.replace("8.18.7", "8 18"),
            FACTS.replace("1790000000", "0"),
            FACTS.replace("1790000000", "4102444801"),
            FACTS.replace("\"ffmpeg\": \"9.1\", ", ""),
            "not json".to_owned(),
        ];

        let accepted: Vec<_> = bad.iter().filter(|text| parse(text).is_ok()).collect();

        assert!(accepted.is_empty(), "{accepted:?}");
    }

    #[test]
    fn a_file_that_is_not_there_is_not_reported() {
        let facts = BuildFacts::load(Path::new("/nonexistent/build-facts.json"));

        assert_eq!(facts, BuildFacts::unreported());
    }
}

use icu_casemap::CaseMapper;
use icu_normalizer::ComposingNormalizerBorrowed;

use crate::name::ValidName;

/// Pinned fold version stored in `nodes.name_key_version` (smallint): Unicode major * 100 + minor.
/// Changing the fold, the normaliser or this value is breaking: stored keys change.
pub const NAME_KEY_VERSION: i16 = 1800;

/// Unicode version of the pinned ICU4X data (`icu_normalizer` and `icu_casemap` 2.4.0, exact pins in
/// the workspace manifest). The `fold_table_digest` test fails when a library bump changes any key.
pub const NAME_KEY_UNICODE_VERSION: &str = "18.0.0";

/// The folded key of a name. Uniqueness per folder is byte equality of keys (`COLLATE "C"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameKey(String);

impl NameKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub(crate) fn from_name_str(s: &str) -> Self {
        Self(fold(s))
    }
}

/// NFC, then Unicode full case folding (default, non-Turkic: `ß` becomes `ss`, `I` becomes `i`, dotless
/// `ı` stays `ı`, `İ` becomes `i` + U+0307), then NFC again: folding can leave a sequence that composes
/// (`ß` + U+0301 folds to `ss` + U+0301, which composes to `sś`), and without the last step the key of a
/// key would differ from the key.
pub fn name_key(name: &ValidName) -> NameKey {
    NameKey::from_name_str(name.as_str())
}

pub(crate) fn nfc(s: &str) -> std::borrow::Cow<'_, str> {
    ComposingNormalizerBorrowed::new_nfc().normalize(s)
}

fn fold(s: &str) -> String {
    let composed = nfc(s);
    let folded = CaseMapper::new().fold_string(&composed);
    nfc(&folded).into_owned()
}

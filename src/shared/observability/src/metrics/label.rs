use crate::error::ObsError;

/// Most labels one family may have.
pub const MAX_LABELS: usize = 4;

/// Label names that stand for unbounded values. A family naming one is refused at construction
/// (`BadLabel`): a label value is a series, and an account, node, file, share or address per series is a leak
/// and a cardinality bomb. A name matches when any of its `_`-separated words is listed.
const UNBOUNDED_WORDS: &[&str] = &[
    "account", "node", "file", "path", "share", "ip", "addr", "address", "user", "email", "token",
    "session", "device", "request", "url", "uri", "host", "name",
];

/// A static enumeration of label values.
///
/// Implemented by `Copy` enums (or small structs of enums) whose [`value`](LabelSet::value) returns a
/// `&'static str` per variant, so a series can only ever carry a value written in the source. A `String`,
/// a `&str` taken from a request or an `Uuid` does not implement it:
///
/// ```compile_fail
/// let build = sc_observability::BuildInfo { version: "0", commit: "0", test_clock: false };
/// let metrics = sc_observability::Metrics::new(build).unwrap();
/// let _ = metrics.counter::<String>("requests_total");
/// ```
///
/// An enumeration implements it with one `&'static str` per variant:
///
/// ```
/// use sc_observability::LabelSet;
///
/// #[derive(Clone, Copy)]
/// enum Kind { Thumbnail, Transcode }
///
/// impl LabelSet for Kind {
///     const NAMES: &'static [&'static str] = &["kind"];
///     fn value(&self, _index: usize) -> &'static str {
///         match self { Kind::Thumbnail => "thumbnail", Kind::Transcode => "transcode" }
///     }
/// }
/// ```
pub trait LabelSet: Copy + Send + Sync + 'static {
    /// Label names, in the order of [`value`](LabelSet::value) indexes. At most [`MAX_LABELS`].
    const NAMES: &'static [&'static str];

    /// Value of the label `NAMES[index]` for this member of the enumeration.
    fn value(&self, index: usize) -> &'static str;
}

/// The label set of a family without labels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoLabels;

impl LabelSet for NoLabels {
    const NAMES: &'static [&'static str] = &[];

    fn value(&self, _index: usize) -> &'static str {
        ""
    }
}

pub(crate) fn values<L: LabelSet>(labels: &L) -> [&'static str; MAX_LABELS] {
    let mut out = [""; MAX_LABELS];
    for (index, slot) in out.iter_mut().enumerate().take(L::NAMES.len()) {
        *slot = labels.value(index);
    }
    out
}

fn bad_label(metric: &'static str, reason: impl Into<String>) -> ObsError {
    ObsError::BadLabel {
        metric,
        reason: reason.into(),
    }
}

pub(crate) fn validate<L: LabelSet>(metric: &'static str) -> Result<(), ObsError> {
    if L::NAMES.len() > MAX_LABELS {
        return Err(bad_label(metric, format!("more than {MAX_LABELS} labels")));
    }
    for (index, name) in L::NAMES.iter().enumerate() {
        if !is_identifier(name) || name.starts_with("__") || *name == "le" || *name == "quantile" {
            return Err(bad_label(
                metric,
                format!("`{name}` is not a usable label name"),
            ));
        }
        if let Some(word) = name.split('_').find(|word| UNBOUNDED_WORDS.contains(word)) {
            return Err(bad_label(
                metric,
                format!(
                    "`{name}` names an unbounded value (`{word}`); labels are enumerations only"
                ),
            ));
        }
        if L::NAMES[..index].contains(name) {
            return Err(bad_label(metric, format!("`{name}` is listed twice")));
        }
    }
    Ok(())
}

pub(crate) fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

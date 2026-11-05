use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use crate::spec::KeySpec;

/// Syntax family of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Text,
    Flag,
    Count,
    Size,
    Duration,
    Address,
    List,
    Environment,
}

impl ValueKind {
    /// Range bounds and values compare as numbers: counts, bytes, milliseconds.
    pub fn parse_number(self, text: &str) -> Option<u64> {
        match self {
            ValueKind::Count => parse_count(text),
            ValueKind::Size => parse_size(text),
            ValueKind::Duration => parse_duration(text).map(duration_millis),
            _ => None,
        }
    }
}

/// How one field type reads itself from text.
pub trait SettingType: Sized + Default {
    const KIND: ValueKind;
    const SECRET: bool = false;
    const OPTIONAL: bool = false;

    /// `None` means the text is malformed for this type.
    fn parse(text: &str, spec: &KeySpec) -> Option<Self>;

    /// The value as a number when the type has a range.
    fn number(&self) -> Option<u64> {
        None
    }
}

fn parse_count(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn split_number_unit(text: &str) -> Option<(u64, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(end);
    Some((parse_count(digits)?, unit))
}

/// `2s`, `5m`, `7d`, `48h`, `500ms`; a bare `0` is a zero duration.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let (n, unit) = split_number_unit(text)?;
    match unit {
        "ms" => Some(Duration::from_millis(n)),
        "s" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_secs(n.checked_mul(60)?)),
        "h" => Some(Duration::from_secs(n.checked_mul(3_600)?)),
        "d" => Some(Duration::from_secs(n.checked_mul(86_400)?)),
        "" if n == 0 => Some(Duration::ZERO),
        _ => None,
    }
}

/// `4MiB`, `50GiB`; a bare integer is bytes.
pub fn parse_size(text: &str) -> Option<u64> {
    let (n, unit) = split_number_unit(text)?;
    let factor: u64 = match unit {
        "" | "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return None,
    };
    n.checked_mul(factor)
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A secret value; `Debug` and `Display` print `***` and nothing else prints it.
#[derive(Clone)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl<T: Default> Default for Secret<T> {
    fn default() -> Self {
        Self(T::default())
    }
}

/// A validated duration key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DurationSetting(Duration);

impl DurationSetting {
    pub fn get(&self) -> Duration {
        self.0
    }
}

/// A validated byte count (`4MiB` reads as 4194304).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size(u64);

impl Size {
    pub fn bytes(&self) -> u64 {
        self.0
    }
}

/// A validated listen address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenAddr(SocketAddr);

impl ListenAddr {
    pub fn get(&self) -> SocketAddr {
        self.0
    }
}

impl Default for ListenAddr {
    fn default() -> Self {
        Self(SocketAddr::from(([0, 0, 0, 0], 0)))
    }
}

/// Deployment environment. Duration floors and ceilings and the `https` rule bind per environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Environment {
    #[default]
    Production,
    Development,
    Test,
}

impl Environment {
    pub fn as_str(self) -> &'static str {
        match self {
            Environment::Production => "production",
            Environment::Development => "development",
            Environment::Test => "test",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "production" => Some(Environment::Production),
            "development" => Some(Environment::Development),
            "test" => Some(Environment::Test),
            _ => None,
        }
    }
}

impl SettingType for String {
    const KIND: ValueKind = ValueKind::Text;

    fn parse(text: &str, spec: &KeySpec) -> Option<Self> {
        (spec.choices.is_empty() || spec.choices.contains(&text)).then(|| text.to_owned())
    }
}

impl SettingType for bool {
    const KIND: ValueKind = ValueKind::Flag;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        match text {
            "true" | "on" => Some(true),
            "false" | "off" => Some(false),
            _ => None,
        }
    }
}

impl SettingType for u64 {
    const KIND: ValueKind = ValueKind::Count;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        parse_count(text)
    }

    fn number(&self) -> Option<u64> {
        Some(*self)
    }
}

impl SettingType for Size {
    const KIND: ValueKind = ValueKind::Size;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        parse_size(text).map(Size)
    }

    fn number(&self) -> Option<u64> {
        Some(self.0)
    }
}

impl SettingType for DurationSetting {
    const KIND: ValueKind = ValueKind::Duration;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        parse_duration(text).map(DurationSetting)
    }

    fn number(&self) -> Option<u64> {
        Some(duration_millis(self.0))
    }
}

impl SettingType for ListenAddr {
    const KIND: ValueKind = ValueKind::Address;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        text.parse().ok().map(ListenAddr)
    }
}

impl SettingType for Environment {
    const KIND: ValueKind = ValueKind::Environment;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        Environment::parse(text)
    }
}

/// Comma-separated; an empty text is an empty list, an empty element is malformed.
impl SettingType for Vec<String> {
    const KIND: ValueKind = ValueKind::List;

    fn parse(text: &str, _: &KeySpec) -> Option<Self> {
        if text.is_empty() {
            return Some(Vec::new());
        }
        text.split(',')
            .map(|item| {
                let item = item.trim();
                (!item.is_empty()).then(|| item.to_owned())
            })
            .collect()
    }
}

impl<T: SettingType> SettingType for Option<T> {
    const KIND: ValueKind = T::KIND;
    const SECRET: bool = T::SECRET;
    const OPTIONAL: bool = true;

    fn parse(text: &str, spec: &KeySpec) -> Option<Self> {
        T::parse(text, spec).map(Some)
    }

    fn number(&self) -> Option<u64> {
        self.as_ref().and_then(T::number)
    }
}

impl<T: SettingType> SettingType for Secret<T> {
    const KIND: ValueKind = T::KIND;
    const SECRET: bool = true;

    fn parse(text: &str, spec: &KeySpec) -> Option<Self> {
        T::parse(text, spec).map(Secret::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_every_unit() {
        let cases = [
            ("2s", Duration::from_secs(2)),
            ("5m", Duration::from_secs(300)),
            ("7d", Duration::from_secs(7 * 86_400)),
            ("48h", Duration::from_secs(48 * 3_600)),
            ("500ms", Duration::from_millis(500)),
            ("0", Duration::ZERO),
            ("0ms", Duration::ZERO),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_duration(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn durations_reject_malformed_text() {
        for text in [
            "",
            "s",
            "5",
            "5x",
            "-1s",
            "+1s",
            "1.5s",
            " 1s",
            "1 s",
            "1S",
            "18446744073709551615d",
        ] {
            assert_eq!(parse_duration(text), None, "{text:?}");
        }
    }

    #[test]
    fn sizes_parse_every_unit() {
        assert_eq!(parse_size("4MiB"), Some(4 << 20));
        assert_eq!(parse_size("16KiB"), Some(16 << 10));
        assert_eq!(parse_size("50GiB"), Some(50 << 30));
        assert_eq!(parse_size("1TiB"), Some(1 << 40));
        assert_eq!(parse_size("250000000"), Some(250_000_000));
        assert_eq!(parse_size("7B"), Some(7));
        for text in [
            "",
            "MiB",
            "4mib",
            "4MB",
            "1.5GiB",
            "18446744073709551615TiB",
        ] {
            assert_eq!(parse_size(text), None, "{text:?}");
        }
    }

    #[test]
    fn secret_never_prints_its_value() {
        let secret = Secret::new(String::from("planted-value"));
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(format!("{secret}"), "***");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(***)");
        assert_eq!(secret.expose(), "planted-value");
    }

    #[test]
    fn huge_durations_saturate_instead_of_wrapping() {
        let huge = DurationSetting(Duration::from_secs(u64::MAX));
        assert_eq!(huge.number(), Some(u64::MAX));
    }

    #[test]
    fn lists_split_on_commas() {
        let spec = crate::spec::KeySpec::without_default(crate::spec::readers::A);
        assert_eq!(
            <Vec<String>>::parse("a, b ,c", &spec),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(<Vec<String>>::parse("", &spec), Some(Vec::new()));
        assert_eq!(<Vec<String>>::parse("a,,b", &spec), None);
        assert_eq!(<Vec<String>>::parse("a,", &spec), None);
    }
}

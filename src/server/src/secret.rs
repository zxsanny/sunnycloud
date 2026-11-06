use std::fmt;

/// A token or key the operator must see exactly once.
///
/// It has no `Clone`, `Display` or serializer, and `Debug` redacts it. The only reader is the output writer,
/// which prints it on stdout and nowhere else.
pub struct OneTimeSecret {
    label: &'static str,
    value: String,
}

impl OneTimeSecret {
    /// `label` names the value in the output (`token`).
    pub fn new(label: &'static str, value: String) -> Self {
        Self { label, value }
    }

    pub(crate) fn reveal(self) -> (&'static str, String) {
        (self.label, self.value)
    }
}

impl fmt::Debug for OneTimeSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OneTimeSecret({}: ***)", self.label)
    }
}

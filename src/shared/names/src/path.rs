use crate::name::{NameError, ValidName, validate_name};

/// Splits a relative, `/`-separated path into validated names. Every component must be a legal name, so
/// `.`, `..`, empty components (leading, trailing or doubled `/`) and the empty path are errors; the
/// result never holds `.`, `..` or a component with a separator. `\` is an ordinary name character.
pub fn split_path(path: &str) -> Result<Vec<ValidName>, NameError> {
    path.split('/').map(validate_name).collect()
}

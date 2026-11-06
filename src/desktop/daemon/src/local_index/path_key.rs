use sc_names::{name_key, split_path};

use super::IndexError;

/// The form of `rel_path` under which entries of a pair are looked up: the path itself on a case-sensitive
/// pair, every component folded by `name_key` on a case-insensitive one (14 §5 step 4).
pub(super) fn path_key(rel_path: &str, case_insensitive: bool) -> Result<String, IndexError> {
    if !case_insensitive {
        return Ok(rel_path.to_owned());
    }
    let keys: Vec<String> = split_path(rel_path)?
        .iter()
        .map(|name| name_key(name).as_str().to_owned())
        .collect();
    Ok(keys.join("/"))
}

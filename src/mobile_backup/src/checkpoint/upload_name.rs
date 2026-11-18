//! The name an asset is uploaded under (17 §4, F-10): its display name, or `<stem> (<asset_id>)<ext>` when another
//! asset already holds that name. The extension rule and the length limit are those of `sc_names`.

use sc_names::{
    COLLISION_SUFFIX_CLOSE, COLLISION_SUFFIX_OPEN, MAX_EXTENSION_BYTES, MAX_NAME_BYTES,
};

/// `IMG_0001.jpg` and id 202 give `IMG_0001 (202).jpg`. The stem is cut on a character boundary so that the result
/// stays within the name limit; the extension is the text from the last dot, when that dot is not the first
/// character and the text is at most `MAX_EXTENSION_BYTES` long.
pub(super) fn with_asset_id(display_name: &str, asset_id: i64) -> String {
    let (stem, extension) = match display_name.rfind('.') {
        Some(dot) if dot > 0 && display_name.len() - dot <= MAX_EXTENSION_BYTES => {
            display_name.split_at(dot)
        }
        _ => (display_name, ""),
    };
    let suffix = format!("{COLLISION_SUFFIX_OPEN}{asset_id}{COLLISION_SUFFIX_CLOSE}");
    let room = MAX_NAME_BYTES.saturating_sub(suffix.len() + extension.len());
    let cut = (0..=room.min(stem.len()))
        .rev()
        .find(|&at| stem.is_char_boundary(at))
        .unwrap_or(0);
    format!("{}{suffix}{extension}", &stem[..cut])
}

#[cfg(test)]
mod tests {
    use sc_names::validate_name;

    use super::with_asset_id;

    #[test]
    fn the_asset_id_goes_between_the_stem_and_the_extension() {
        // Arrange
        let name = "IMG_0001.jpg";

        // Act
        let chosen = with_asset_id(name, 202);

        // Assert
        assert_eq!(chosen, "IMG_0001 (202).jpg");
    }

    #[test]
    fn a_name_without_an_extension_gets_the_suffix_at_its_end() {
        // Arrange
        let names = ["README", ".hidden"];

        // Act
        let chosen: Vec<_> = names.iter().map(|name| with_asset_id(name, 7)).collect();

        // Assert
        assert_eq!(chosen, ["README (7)", ".hidden (7)"]);
    }

    #[test]
    fn a_name_at_the_length_limit_stays_a_valid_name_after_the_suffix() {
        // Arrange
        let name = format!("{}.jpg", "é".repeat(125));

        // Act
        let chosen = with_asset_id(&name, 9_223_372_036_854_775_807);

        // Assert
        assert!(validate_name(&chosen).is_ok(), "{chosen}");
        assert!(chosen.ends_with(" (9223372036854775807).jpg"), "{chosen}");
    }
}

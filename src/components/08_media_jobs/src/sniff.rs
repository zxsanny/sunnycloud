//! The media type of a worker output, read from its first bytes. The worker's `Content-Type` is never used.

/// Bytes of the stored plaintext the sniffer reads.
pub(crate) const HEAD_LEN: u32 = 32;

pub(crate) const WEBP: &str = "image/webp";
pub(crate) const JPEG: &str = "image/jpeg";
pub(crate) const MP4: &str = "video/mp4";

/// The VP8 chunk that must follow the RIFF/WEBP header, so twelve copied header bytes alone do not pass.
const WEBP_CHUNKS: [&[u8; 4]; 3] = [b"VP8 ", b"VP8L", b"VP8X"];

/// ISO base media major brands of an MP4. QuickTime (`qt  `), HEIF/HEIC and AVIF brands share the `ftyp` box but
/// are not MP4 and are refused.
const MP4_BRANDS: [&[u8; 4]; 11] = [
    b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1", b"M4V ",
    b"dash",
];

/// `image/webp`, `image/jpeg`, `video/mp4`, or `None` for anything else.
pub(crate) fn sniff(head: &[u8]) -> Option<&'static str> {
    if is_webp(head) {
        Some(WEBP)
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(JPEG)
    } else if is_mp4(head) {
        Some(MP4)
    } else {
        None
    }
}

fn is_webp(head: &[u8]) -> bool {
    head.len() >= 16
        && head.starts_with(b"RIFF")
        && &head[8..12] == b"WEBP"
        && WEBP_CHUNKS.iter().any(|chunk| &head[12..16] == *chunk)
}

/// The first box is `ftyp`, at least the 16 bytes of size, type, major brand and minor version.
fn is_mp4(head: &[u8]) -> bool {
    if head.len() < 16 || &head[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    size >= 16 && MP4_BRANDS.iter().any(|brand| &head[8..12] == *brand)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEBP_HEAD: &[u8] = b"RIFF\x1a\x00\x00\x00WEBPVP8L\x0d\x00\x00\x00";
    const JPEG_HEAD: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00";
    const MP4_HEAD: &[u8] = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2";
    const PNG_HEAD: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
    const SVG_HEAD: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\">";
    const TEXT_HEAD: &[u8] = b"this is not a video at all, honest";

    #[test]
    fn webp_jpeg_and_mp4_heads_are_recognised() {
        // Arrange
        let heads = [WEBP_HEAD, JPEG_HEAD, MP4_HEAD];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(types, [Some(WEBP), Some(JPEG), Some(MP4)]);
    }

    #[test]
    fn png_svg_text_and_empty_heads_have_no_accepted_type() {
        // Arrange
        let heads: [&[u8]; 4] = [PNG_HEAD, SVG_HEAD, TEXT_HEAD, b""];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(types, [None, None, None, None]);
    }

    #[test]
    fn a_riff_header_without_a_vp8_chunk_is_not_webp() {
        // Arrange
        let html = b"RIFF\x00\x00\x00\x00WEBP<html><body>";
        let wave = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00";

        // Act
        let types = [sniff(html), sniff(wave)];

        // Assert
        assert_eq!(types, [None, None]);
    }

    #[test]
    fn quicktime_heic_and_short_ftyp_boxes_are_not_mp4() {
        // Arrange
        let quicktime = b"\x00\x00\x00\x14ftypqt  \x00\x00\x02\x00qt  ";
        let heic = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic";
        let short_box = b"\x00\x00\x00\x08ftypisom\x00\x00\x02\x00";
        let truncated = b"\x00\x00\x00\x20ftypisom";

        // Act
        let types = [
            sniff(quicktime),
            sniff(heic),
            sniff(short_box),
            sniff(truncated),
        ];

        // Assert
        assert_eq!(types, [None, None, None, None]);
    }
}

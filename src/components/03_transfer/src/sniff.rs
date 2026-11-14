//! The media type of an upload, read from the head of its plaintext at commit (AC-47). The file name and any
//! client-declared type are never consulted; an unrecognised head has no type and is served as an attachment.
//! JPEG, WebP and MP4 follow the rules of 08's worker-output check (owner decision 2026-11-14: own sniffer, no
//! shared helper). Recognised: the inline allowlist (JPEG, PNG, GIF, WebP, AVIF, MP4, WebM), the other types 08
//! plans jobs for (HEIC, QuickTime, Matroska), HTML and PDF.

/// The most bytes `BlobReader::read_head` returns, and what commit asks for.
pub(crate) const HEAD_LEN: u32 = 64 * 1024;

const JPEG: &str = "image/jpeg";
const PNG: &str = "image/png";
const GIF: &str = "image/gif";
const WEBP: &str = "image/webp";
const AVIF: &str = "image/avif";
const HEIC: &str = "image/heic";
const MP4: &str = "video/mp4";
const QUICKTIME: &str = "video/quicktime";
const WEBM: &str = "video/webm";
const MATROSKA: &str = "video/x-matroska";
const HTML: &str = "text/html";
const PDF: &str = "application/pdf";

/// The VP8 chunk that must follow the RIFF/WEBP header, so twelve copied header bytes alone do not pass.
const WEBP_CHUNKS: [&[u8; 4]; 3] = [b"VP8 ", b"VP8L", b"VP8X"];

/// ISO base media major brands of an MP4 (as 08). QuickTime and the HEIF family share the `ftyp` box.
const MP4_BRANDS: [&[u8; 4]; 11] = [
    b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1", b"M4V ",
    b"dash",
];
const AVIF_BRANDS: [&[u8; 4]; 2] = [b"avif", b"avis"];
const HEIC_BRANDS: [&[u8; 4]; 6] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx"];
/// Generic HEIF major brands: the compatible brands tell AVIF from HEIC.
const HEIF_BRANDS: [&[u8; 4]; 2] = [b"mif1", b"msf1"];

const EBML_MAGIC: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];
/// The EBML `DocType` element id.
const DOC_TYPE_ID: [u8; 2] = [0x42, 0x82];

/// The tags of the WHATWG MIME sniffing HTML pattern; a match needs a space or `>` after the tag.
const HTML_TAGS: [&[u8]; 17] = [
    b"<!doctype html",
    b"<html",
    b"<head",
    b"<script",
    b"<iframe",
    b"<h1",
    b"<div",
    b"<font",
    b"<table",
    b"<a",
    b"<style",
    b"<title",
    b"<b",
    b"<body",
    b"<br",
    b"<p",
    b"<!--",
];

/// The media type the head's bytes show, `None` when they show none of the recognised types.
pub(crate) fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(JPEG)
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(PNG)
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some(GIF)
    } else if is_webp(head) {
        Some(WEBP)
    } else if head.starts_with(b"%PDF-") {
        Some(PDF)
    } else if let Some(found) = iso_media(head) {
        Some(found)
    } else if let Some(found) = matroska(head) {
        Some(found)
    } else if is_html(head) {
        Some(HTML)
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

/// A first `ftyp` box of at least the 16 bytes of size, type, major brand and minor version, typed by its major
/// brand; a generic HEIF brand is typed by the compatible brands inside the box.
fn iso_media(head: &[u8]) -> Option<&'static str> {
    if head.len() < 16 || &head[4..8] != b"ftyp" {
        return None;
    }
    let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if size < 16 {
        return None;
    }
    let major = &head[8..12];
    let is = |brands: &[&[u8; 4]]| brands.iter().any(|brand| major == *brand);
    if is(&MP4_BRANDS) {
        Some(MP4)
    } else if major == b"qt  " {
        Some(QUICKTIME)
    } else if is(&AVIF_BRANDS) {
        Some(AVIF)
    } else if is(&HEIC_BRANDS) {
        Some(HEIC)
    } else if is(&HEIF_BRANDS) {
        let compatible = head.get(16..size.min(head.len())).unwrap_or_default();
        let has = |brands: &[&[u8; 4]]| {
            compatible
                .chunks_exact(4)
                .any(|brand| brands.iter().any(|known| brand == *known))
        };
        if has(&AVIF_BRANDS) {
            Some(AVIF)
        } else if has(&HEIC_BRANDS) {
            Some(HEIC)
        } else {
            None
        }
    } else {
        None
    }
}

/// An EBML header whose `DocType` (one-byte size) is `webm` or `matroska`.
fn matroska(head: &[u8]) -> Option<&'static str> {
    if !head.starts_with(&EBML_MAGIC) {
        return None;
    }
    let header = &head[..head.len().min(64)];
    let at = header
        .windows(2)
        .position(|pair| pair == DOC_TYPE_ID)?
        .checked_add(2)?;
    let size_byte = *header.get(at)?;
    if size_byte & 0x80 == 0 {
        return None;
    }
    let len = usize::from(size_byte & 0x7f);
    let doc_type = header.get(at + 1..at + 1 + len)?;
    match doc_type {
        b"webm" => Some(WEBM),
        b"matroska" => Some(MATROSKA),
        _ => None,
    }
}

/// After leading whitespace (and a UTF-8 BOM), one of [`HTML_TAGS`] in any case, ended by a space or `>`.
fn is_html(head: &[u8]) -> bool {
    let text = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let start = text
        .iter()
        .position(|byte| !matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' '))
        .unwrap_or(text.len());
    let text = &text[start..];
    HTML_TAGS.iter().any(|tag| {
        text.len() > tag.len()
            && text[..tag.len()].eq_ignore_ascii_case(tag)
            && matches!(text[tag.len()], b' ' | b'>')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG_HEAD: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00";
    const PNG_HEAD: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
    const GIF_HEAD: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00";
    const WEBP_HEAD: &[u8] = b"RIFF\x1a\x00\x00\x00WEBPVP8L\x0d\x00\x00\x00";
    const AVIF_HEAD: &[u8] = b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf";
    const MP4_HEAD: &[u8] = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2";
    const WEBM_HEAD: &[u8] =
        b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\xf7\x81\x01\x42\x82\x84webm\x42\x87";

    #[test]
    fn the_inline_allowlist_is_recognised_from_bytes() {
        // Arrange
        let heads = [
            JPEG_HEAD, PNG_HEAD, GIF_HEAD, WEBP_HEAD, AVIF_HEAD, MP4_HEAD, WEBM_HEAD,
        ];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(
            types,
            [
                Some(JPEG),
                Some(PNG),
                Some(GIF),
                Some(WEBP),
                Some(AVIF),
                Some(MP4),
                Some(WEBM)
            ]
        );
    }

    #[test]
    fn the_other_types_08_plans_jobs_for_are_recognised() {
        // Arrange
        let heic: &[u8] = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic";
        let generic_heic: &[u8] = b"\x00\x00\x00\x18ftypmif1\x00\x00\x00\x00mif1heic";
        let quicktime: &[u8] = b"\x00\x00\x00\x14ftypqt  \x00\x00\x02\x00qt  ";
        let mkv: &[u8] = b"\x1a\x45\xdf\xa3\xa3\x42\x86\x81\x01\x42\x82\x88matroska\x42\x87";

        // Act
        let types = [
            sniff(heic),
            sniff(generic_heic),
            sniff(quicktime),
            sniff(mkv),
        ];

        // Assert
        assert_eq!(
            types,
            [Some(HEIC), Some(HEIC), Some(QUICKTIME), Some(MATROSKA)]
        );
    }

    #[test]
    fn html_is_recognised_in_any_case_after_whitespace_and_a_bom() {
        // Arrange
        let heads: [&[u8]; 4] = [
            b"<!DOCTYPE html><html><body>x</body></html>",
            b"\xef\xbb\xbf  \n<HTML>\n<head>",
            b"<script>alert(1)</script>",
            b"\t<p class=x>hi</p>",
        ];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(types, [Some(HTML); 4]);
    }

    #[test]
    fn hostile_and_unknown_heads_never_get_an_inline_type() {
        // Arrange: DS-17 negatives besides HTML, and random and empty bytes.
        let svg: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\">";
        let xml: &[u8] = b"<?xml version=\"1.0\"?><note/>";
        let js: &[u8] = b"function f() { return document.cookie; }";
        let random: &[u8] = b"\x13\x9a\x07\xee\x41\x00\x9f\x12\x88\x02\x7c\x55\x01\xd4\xe0\x3b";
        let heads = [svg, xml, js, random, b""];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(types, [None, None, None, None, None]);
    }

    #[test]
    fn pdf_is_its_own_type() {
        // Arrange
        let head = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3";

        // Act
        let found = sniff(head);

        // Assert
        assert_eq!(found, Some(PDF));
    }

    #[test]
    fn webp_and_mp4_follow_the_rules_of_the_worker_output_check() {
        // Arrange
        let riff_html: &[u8] = b"RIFF\x00\x00\x00\x00WEBP<html><body>";
        let wave: &[u8] = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00";
        let short_box: &[u8] = b"\x00\x00\x00\x08ftypisom\x00\x00\x02\x00";
        let truncated: &[u8] = b"\x00\x00\x00\x20ftypisom";

        // Act
        let types = [
            sniff(riff_html),
            sniff(wave),
            sniff(short_box),
            sniff(truncated),
        ];

        // Assert
        assert_eq!(types, [None, None, None, None]);
    }

    #[test]
    fn a_tag_prefix_without_its_end_is_not_html() {
        // Arrange
        let heads: [&[u8]; 3] = [b"<abbr>", b"<bodyguard", b"<p"];

        // Act
        let types: Vec<Option<&str>> = heads.iter().map(|head| sniff(head)).collect();

        // Assert
        assert_eq!(types, [None, None, None]);
    }
}

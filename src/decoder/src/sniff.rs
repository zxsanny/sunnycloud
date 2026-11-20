//! Magic-byte sniffing (10 section 4): the loader is chosen from the bytes, never from the file
//! name. An SVG or a PDF named `.png` is [`Sniffed::Unknown`] and no loader runs on it.

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;

/// Bytes the sniffer needs: three MPEG-TS sync bytes 188 apart.
pub const HEAD_LEN: usize = 512;

/// `VIPS_BLOCK_UNTRUSTED=1` for the formats whose loaders are all trusted (A); the HEIF family
/// runs with the variable unset (B), the premise of 10 section 4 step 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    Still {
        profile: Profile,
        format: &'static str,
    },
    /// A video container: the poster path, never a vips loader.
    Video,
    Unknown,
}

const HEIF_BRANDS: [&[u8; 4]; 12] = [
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1",
    b"avif", b"avis",
];
const TS_PACKET: usize = 188;

/// The first [`HEAD_LEN`] bytes of `file` (fewer when it is shorter), read without moving the
/// file offset.
pub fn read_head(file: &File) -> io::Result<Vec<u8>> {
    let mut head = vec![0; HEAD_LEN];
    let mut filled = 0;
    while filled < head.len() {
        let read = file.read_at(&mut head[filled..], filled as u64)?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    head.truncate(filled);
    Ok(head)
}

pub fn sniff(head: &[u8]) -> Sniffed {
    let still = |profile, format| Sniffed::Still { profile, format };
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        still(Profile::A, "jpeg")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        still(Profile::A, "png")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        still(Profile::A, "gif")
    } else if riff_form(head) == Some(b"WEBP") {
        still(Profile::A, "webp")
    } else if riff_form(head) == Some(b"AVI ") || head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Sniffed::Video
    } else if head.get(4..8) == Some(b"ftyp") {
        match head.get(8..12) {
            Some(brand) if HEIF_BRANDS.iter().any(|known| known.as_slice() == brand) => {
                still(Profile::B, "heif")
            }
            _ => Sniffed::Video,
        }
    } else if is_transport_stream(head) {
        Sniffed::Video
    } else {
        Sniffed::Unknown
    }
}

fn riff_form(head: &[u8]) -> Option<&[u8; 4]> {
    if head.starts_with(b"RIFF") {
        head.get(8..12)?.try_into().ok()
    } else {
        None
    }
}

fn is_transport_stream(head: &[u8]) -> bool {
    head.len() > 2 * TS_PACKET
        && [0, TS_PACKET, 2 * TS_PACKET]
            .iter()
            .all(|&at| head[at] == 0x47)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padded(prefix: &[u8]) -> Vec<u8> {
        let mut bytes = prefix.to_vec();
        bytes.resize(HEAD_LEN, 0);
        bytes
    }

    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 0x18];
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(brand);
        padded(&bytes)
    }

    fn riff(form: &[u8; 4]) -> Vec<u8> {
        let mut bytes = b"RIFF\x10\0\0\0".to_vec();
        bytes.extend_from_slice(form);
        padded(&bytes)
    }

    #[test]
    fn each_trusted_still_format_runs_under_profile_a() {
        let cases = [
            (padded(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpeg"),
            (
                padded(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
                "png",
            ),
            (riff(b"WEBP"), "webp"),
            (padded(b"GIF89a"), "gif"),
            (padded(b"GIF87a"), "gif"),
        ];

        for (head, format) in cases {
            assert_eq!(
                sniff(&head),
                Sniffed::Still {
                    profile: Profile::A,
                    format
                },
                "{format}"
            );
        }
    }

    #[test]
    fn heif_brands_run_under_profile_b() {
        for brand in [b"heic", b"heix", b"mif1", b"avif"] {
            let sniffed = sniff(&ftyp(brand));

            assert_eq!(
                sniffed,
                Sniffed::Still {
                    profile: Profile::B,
                    format: "heif"
                },
                "{}",
                String::from_utf8_lossy(brand)
            );
        }
    }

    #[test]
    fn video_containers_take_the_poster_path() {
        let mut ts = vec![0u8; HEAD_LEN];
        for at in [0, 188, 376] {
            ts[at] = 0x47;
        }
        let cases = [
            ftyp(b"isom"),
            ftyp(b"mp42"),
            ftyp(b"qt  "),
            padded(&[0x1A, 0x45, 0xDF, 0xA3]),
            riff(b"AVI "),
            ts,
        ];

        for head in cases {
            assert_eq!(sniff(&head), Sniffed::Video);
        }
    }

    #[test]
    fn svg_pdf_text_and_empty_input_are_unknown() {
        let cases = [
            padded(b"<?xml version=\"1.0\"?><svg xmlns="),
            padded(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            padded(b"%PDF-1.7"),
            padded(b"#EXTM3U\n#EXT-X-VERSION:3\nfile:///etc/passwd"),
            riff(b"WAVE"),
            vec![],
        ];

        for head in cases {
            assert_eq!(sniff(&head), Sniffed::Unknown);
        }
    }

    #[test]
    fn a_lone_transport_sync_byte_is_not_a_stream() {
        let mut head = vec![0u8; HEAD_LEN];
        head[0] = 0x47;

        assert_eq!(sniff(&head), Sniffed::Unknown);
    }
}

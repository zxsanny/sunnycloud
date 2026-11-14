//! ST-03-4 (AZ-3758 AC-4; AC-47): the DS-17 hostile files and positive controls, each committed under a benign
//! and a hostile name and downloaded. The decision comes from the bytes only: every hostile file is an
//! `Attachment`, every positive control `Inline`, whatever the name says.

use contracts::Disposition;

use crate::start_support::Rig;

/// DS-17 hostile files (`gen_hostile.py` shapes): html, svg with a script, xml, js, pdf, a GIF89a/HTML polyglot
/// and an SVG with `onload`.
const HOSTILE: [(&str, &[u8]); 7] = [
    (
        "html",
        b"<!DOCTYPE html><html><body><script>alert(1)</script></body></html>",
    ),
    (
        "svg-script",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
    ),
    ("xml", b"<?xml version=\"1.0\"?><note><to>x</to></note>"),
    ("js", b"fetch('/api/x', {credentials: 'include'});\n"),
    (
        "pdf",
        b"%PDF-1.7\n1 0 obj<</Type/Catalog>>endobj\ntrailer<<>>\n%%EOF\n",
    ),
    (
        "gif-html-polyglot",
        b"GIF89a/*\x00\x00\x00\x00*/=1;<html><body><script>alert(1)</script></body></html>",
    ),
    (
        "svg-onload",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>",
    ),
];

/// DS-17 positive controls: the smallest heads of jpg, png, webp, gif and mp4.
const POSITIVE: [(&str, &[u8]); 5] = [
    ("jpg", b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00\xff\xd9"),
    ("png", b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00"),
    ("webp", b"RIFF\x1a\x00\x00\x00WEBPVP8L\x0d\x00\x00\x00\x2f\x00\x00\x00\x10\x07\x10\x11\x11\x88\x88\xfe\x07\x00"),
    ("gif", b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;"),
    ("mp4", b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41\x00\x00\x00\x08free"),
];

/// Commits `data` under `name` and returns how it is served.
async fn served(rig: &Rig, name: &str, data: &[u8]) -> (Disposition, Option<String>) {
    let committed = rig
        .commit_bytes(name, name, data.to_vec(), None)
        .await
        .expect("committed");
    let download = rig.open_current(committed.node_id).await.expect("served");
    (download.disposition, download.media_type)
}

#[tokio::test]
async fn downloads_of_hostile_files_are_attachments_and_positives_inline_whatever_the_name() {
    // Arrange
    let rig = Rig::new().await;
    let mut hostile = Vec::new();
    let mut positive = Vec::new();

    // Act
    for (kind, data) in HOSTILE {
        for name in [format!("{kind}-a.jpg"), format!("{kind}-a.html")] {
            hostile.push((name.clone(), served(&rig, &name, data).await));
        }
    }
    for (kind, data) in POSITIVE {
        for name in [format!("{kind}-a.{kind}"), format!("{kind}-a.html")] {
            positive.push((name.clone(), served(&rig, &name, data).await));
        }
    }

    // Assert
    let inline_hostile: Vec<_> = hostile
        .iter()
        .filter(|(_, (disposition, _))| *disposition != Disposition::Attachment)
        .collect();
    let attached_positive: Vec<_> = positive
        .iter()
        .filter(|(_, (disposition, _))| *disposition != Disposition::Inline)
        .collect();
    assert_eq!(hostile.len(), 14);
    assert!(inline_hostile.is_empty(), "{inline_hostile:?}");
    assert_eq!(positive.len(), 10);
    assert!(attached_positive.is_empty(), "{attached_positive:?}");
    rig.finish().await;
}

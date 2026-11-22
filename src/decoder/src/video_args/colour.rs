//! The colour handling of an SDR transcode: which inputs need a BT.709 conversion and the zscale
//! step that does it.

use media_shared::policy::VideoStream;

/// The colour facts of an SDR input that make a BT.709 conversion necessary, as zscale names.
/// The names come from ffprobe text of an untrusted file, so only the tokens listed here ever
/// reach an argument; anything else leaves the clip retagged only.
const PRIMARIES: [&str; 5] = ["bt709", "bt470bg", "smpte170m", "smpte240m", "bt2020"];
const TRANSFERS: [&str; 7] = [
    "bt709",
    "smpte170m",
    "smpte240m",
    "bt470bg",
    "iec61966-2-1",
    "bt2020-10",
    "bt2020-12",
];
const MATRICES: [&str; 5] = ["bt709", "bt470bg", "smpte170m", "smpte240m", "bt2020nc"];

/// What a field that the file does not tag is taken to be, by the family its other tags name.
struct Family {
    primaries: &'static str,
    transfer: &'static str,
    matrix: &'static str,
}

const BT709: Family = Family {
    primaries: "bt709",
    transfer: "bt709",
    matrix: "bt709",
};

fn family_of(matrix: Option<&str>, primaries: Option<&str>) -> Family {
    fn named(name: Option<&str>) -> Option<&str> {
        name.filter(|name| *name != "bt709")
    }
    match named(matrix).or(named(primaries)) {
        Some("smpte170m") => Family {
            primaries: "smpte170m",
            transfer: "smpte170m",
            matrix: "smpte170m",
        },
        Some("bt470bg") => Family {
            primaries: "bt470bg",
            transfer: "smpte170m",
            matrix: "bt470bg",
        },
        Some("smpte240m") => Family {
            primaries: "smpte240m",
            transfer: "smpte240m",
            matrix: "smpte240m",
        },
        Some("bt2020nc" | "bt2020") => Family {
            primaries: "bt2020",
            transfer: "bt2020-10",
            matrix: "bt2020nc",
        },
        _ => BT709,
    }
}

/// The zscale step that converts an SDR input to BT.709 (primaries, transfer and matrix, limited
/// range like the HDR chain), or `None` when the clip needs none: every tag it has is BT.709 or it
/// has none (phone clips often have none, and zscale refuses an unspecified source), or it names a
/// value outside the lists above (the clip is then only retagged).
pub(crate) fn sdr_conversion(video: &VideoStream) -> Option<String> {
    fn tag(value: &Option<String>) -> Option<&str> {
        value
            .as_deref()
            .filter(|name| !matches!(*name, "unknown" | "unspecified"))
    }
    let (primaries, transfer, matrix) = (
        tag(&video.primaries),
        tag(&video.transfer),
        tag(&video.matrix),
    );
    let known = |value: Option<&str>, list: &[&str]| value.is_none_or(|name| list.contains(&name));
    if !known(primaries, &PRIMARIES) || !known(transfer, &TRANSFERS) || !known(matrix, &MATRICES) {
        return None;
    }
    if [primaries, transfer, matrix]
        .into_iter()
        .flatten()
        .all(|name| name == "bt709")
    {
        return None;
    }
    let family = family_of(matrix, primaries);
    Some(format!(
        "zscale=pin={}:tin={}:min={}:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p",
        primaries.unwrap_or(family.primaries),
        transfer.unwrap_or(family.transfer),
        matrix.unwrap_or(family.matrix),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(
        primaries: Option<&str>,
        transfer: Option<&str>,
        matrix: Option<&str>,
    ) -> VideoStream {
        let text = |value: Option<&str>| value.map(str::to_owned);
        VideoStream {
            codec: Some("h264".to_owned()),
            profile: None,
            width: 640,
            height: 360,
            rotation: None,
            bitrate: None,
            pix_fmt: None,
            primaries: text(primaries),
            transfer: text(transfer),
            matrix: text(matrix),
        }
    }

    #[test]
    fn bt709_and_untagged_sdr_input_gets_no_conversion() {
        let cases = [
            tagged(Some("bt709"), Some("bt709"), Some("bt709")),
            tagged(None, None, None),
            tagged(Some("unknown"), Some("unknown"), Some("unspecified")),
            tagged(None, None, Some("bt709")),
        ];

        for video in cases {
            assert_eq!(sdr_conversion(&video), None, "{video:?}");
        }
    }

    #[test]
    fn a_bt601_tagged_input_is_converted_to_bt709_with_limited_range() {
        let ntsc = tagged(Some("smpte170m"), Some("smpte170m"), Some("smpte170m"));
        let pal = tagged(Some("bt470bg"), Some("smpte170m"), Some("bt470bg"));

        assert_eq!(
            sdr_conversion(&ntsc).as_deref(),
            Some(
                "zscale=pin=smpte170m:tin=smpte170m:min=smpte170m:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p"
            )
        );
        assert_eq!(
            sdr_conversion(&pal).as_deref(),
            Some(
                "zscale=pin=bt470bg:tin=smpte170m:min=bt470bg:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p"
            )
        );
    }

    #[test]
    fn an_untagged_field_takes_the_value_of_the_family_the_other_tags_name() {
        let matrix_only = tagged(None, None, Some("smpte170m"));
        let primaries_only = tagged(Some("bt2020"), None, None);

        assert_eq!(
            sdr_conversion(&matrix_only).as_deref(),
            Some(
                "zscale=pin=smpte170m:tin=smpte170m:min=smpte170m:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p"
            )
        );
        assert_eq!(
            sdr_conversion(&primaries_only).as_deref(),
            Some(
                "zscale=pin=bt2020:tin=bt2020-10:min=bt2020nc:p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p"
            )
        );
    }

    #[test]
    fn a_tag_outside_the_lists_never_reaches_an_argument() {
        let hostile = tagged(Some("smpte170m"), Some("x:y=1,movie=/etc/passwd"), None);
        let hdr = tagged(Some("bt2020"), Some("smpte2084"), Some("bt2020nc"));

        assert_eq!(sdr_conversion(&hostile), None);
        assert_eq!(sdr_conversion(&hdr), None);
    }
}

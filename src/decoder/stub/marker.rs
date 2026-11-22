//! The marker that selects the behaviour of the test stub (10 description 13.4): the text
//! `SCSTUB1 <behaviour> [argument]` inside a JPEG comment segment (image jobs) or an MP4 `free`
//! box after `ftyp` (video jobs). The stub reads the first 4 KiB of its input; the container
//! suite writes the same text with [`Behaviour::jpeg`] and [`Behaviour::mp4`], so the two sides
//! cannot drift.

/// The text that opens a marker.
pub const TAG: &str = "SCSTUB1 ";
/// How much of the input the stub looks at.
pub const SCAN_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Behaviour {
    /// Sleep until killed.
    Hang,
    /// Touch this many MiB of memory, then sleep.
    Alloc(u64),
    /// Ignore SIGTERM, then sleep.
    IgnoreTerm,
    /// `setsid`, double fork, and in the descendant loop opening the next job's `in/source`
    /// (the job directory of this id), appending `LEAK` to `<scratch>/leak.txt` on success.
    SetsidFork(String),
    /// Try to open the next job's `in/source` for writing and record the answer in `out/`.
    WriteNextJob(String),
    /// `ptrace`, `process_vm_readv` and `/proc` reads aimed at the runner; errno values go to `out/`.
    Ptrace,
    /// Write this many KiB to stderr, then sleep.
    Stderr(u64),
    /// Write this many MiB to `out/result.webp`.
    Write(u64),
    /// Busy loop on every core.
    Spin,
    /// Die by SIGSEGV.
    Segv,
    /// Exit with this code.
    Exit(i32),
}

impl Behaviour {
    /// The marker text after [`TAG`].
    pub fn text(&self) -> String {
        match self {
            Behaviour::Hang => "hang".into(),
            Behaviour::Alloc(mib) => format!("alloc {mib}"),
            Behaviour::IgnoreTerm => "ignore-term".into(),
            Behaviour::SetsidFork(job) => format!("setsid-fork {job}"),
            Behaviour::WriteNextJob(job) => format!("write-next-job {job}"),
            Behaviour::Ptrace => "ptrace".into(),
            Behaviour::Stderr(kib) => format!("stderr {kib}"),
            Behaviour::Write(mib) => format!("write {mib}"),
            Behaviour::Spin => "spin".into(),
            Behaviour::Segv => "segv".into(),
            Behaviour::Exit(code) => format!("exit {code}"),
        }
    }

    /// A file the runner sniffs as a JPEG, with the marker in a comment segment.
    pub fn jpeg(&self) -> Vec<u8> {
        let text = format!("{TAG}{}", self.text());
        let length = u16::try_from(text.len() + 2).expect("a marker is short");
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xFE];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    /// A file the runner sniffs as a video container, with the marker in a `free` box.
    pub fn mp4(&self) -> Vec<u8> {
        let ftyp: &[u8] = b"\0\0\0\x14ftypisom\0\0\0\0isom";
        let text = format!("{TAG}{}", self.text());
        let size = u32::try_from(text.len() + 8).expect("a marker is short");
        let mut bytes = ftyp.to_vec();
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(b"free");
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }
}

/// The behaviour a marker in `head` selects; `None` when there is no marker or it is not one
/// of the known behaviours.
pub fn parse(head: &[u8]) -> Option<Behaviour> {
    let head = &head[..head.len().min(SCAN_LEN)];
    let start = head
        .windows(TAG.len())
        .position(|window| window == TAG.as_bytes())?
        + TAG.len();
    let text: String = head[start..]
        .iter()
        .take_while(|byte| (0x20..0x7f).contains(*byte))
        .map(|&byte| char::from(byte))
        .collect();
    let mut words = text.split_whitespace();
    let name = words.next()?;
    let argument = words.next();
    let number = || argument?.parse::<u64>().ok();
    match name {
        "hang" => Some(Behaviour::Hang),
        "alloc" => number().map(Behaviour::Alloc),
        "ignore-term" => Some(Behaviour::IgnoreTerm),
        "setsid-fork" => argument.map(|job| Behaviour::SetsidFork(job.to_owned())),
        "write-next-job" => argument.map(|job| Behaviour::WriteNextJob(job.to_owned())),
        "ptrace" => Some(Behaviour::Ptrace),
        "stderr" => number().map(Behaviour::Stderr),
        "write" => number().map(Behaviour::Write),
        "spin" => Some(Behaviour::Spin),
        "segv" => Some(Behaviour::Segv),
        "exit" => argument?.parse().ok().map(Behaviour::Exit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_behaviour() -> Vec<Behaviour> {
        vec![
            Behaviour::Hang,
            Behaviour::Alloc(512),
            Behaviour::IgnoreTerm,
            Behaviour::SetsidFork("job-b".into()),
            Behaviour::WriteNextJob("job-b".into()),
            Behaviour::Ptrace,
            Behaviour::Stderr(1024),
            Behaviour::Write(1024),
            Behaviour::Spin,
            Behaviour::Segv,
            Behaviour::Exit(7),
        ]
    }

    #[test]
    fn each_marker_selects_its_behaviour_in_a_jpeg_and_in_an_mp4() {
        for behaviour in every_behaviour() {
            assert_eq!(parse(&behaviour.jpeg()), Some(behaviour.clone()));
            assert_eq!(parse(&behaviour.mp4()), Some(behaviour));
        }
    }

    #[test]
    fn a_jpeg_marker_sniffs_as_a_jpeg_and_an_mp4_marker_as_a_video() {
        assert!(Behaviour::Hang.jpeg().starts_with(&[0xFF, 0xD8, 0xFF]));
        assert_eq!(&Behaviour::Hang.mp4()[4..8], b"ftyp");
    }

    #[test]
    fn input_without_a_marker_or_with_an_unknown_one_selects_nothing() {
        assert_eq!(parse(b"\xFF\xD8\xFF\xE0 ordinary jpeg"), None);
        assert_eq!(parse(b"SCSTUB1 dance"), None);
        assert_eq!(parse(b"SCSTUB1 alloc many"), None);
        assert_eq!(parse(b"SCSTUB1 setsid-fork"), None);
        assert_eq!(parse(b""), None);
    }

    #[test]
    fn a_marker_after_the_first_4_kib_is_not_seen() {
        let mut bytes = vec![0; SCAN_LEN];
        bytes.extend_from_slice(b"SCSTUB1 hang");

        assert_eq!(parse(&bytes), None);
    }
}

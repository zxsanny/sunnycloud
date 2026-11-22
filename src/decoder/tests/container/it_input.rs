//! IT-10-17: `in/source` and `job_dir` are opened without following links and only as regular files.

use std::ffi::CString;
use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{DecodeError, Request, Response};

use crate::decoder::{Decoder, Flavor, Job, release_image};
use crate::peer::{Peer, photo_limits, render};
use crate::refs::{pattern_jpeg, workdir};

const WAIT: Duration = Duration::from_secs(30);

fn refused(peer: &mut Peer, request: Request, what: &str) {
    let started = Instant::now();

    let answer = peer.call(request, WAIT);

    assert_eq!(answer, Response::Error(DecodeError::Corrupt), "{what}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{what}: answered after {:?}",
        started.elapsed()
    );
}

fn control(peer: &mut Peer, job: &Job) {
    let answer = peer.call(render(job, 320, photo_limits()), WAIT);
    assert!(
        matches!(answer, Response::ImageResult(_)),
        "the control job after a refusal: {answer:?}"
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_17_a_source_that_is_not_a_plain_file_and_a_job_dir_outside_the_tree_are_refused() {
    let work = workdir();
    let photo = pattern_jpeg(work.path(), "p.jpg", 800, 600);
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let mut peer = decoder.connect();

    // `in/source` as a FIFO, a symlink to /etc/passwd, a hard link (nlink 2) and a directory
    let fifo = decoder.empty_job();
    let fifo_path = CString::new(fifo.source().to_str().expect("utf8")).expect("no NUL");
    // SAFETY: `mkfifo` takes a valid C string and a mode; nothing else is involved.
    assert_eq!(
        unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o640) },
        0,
        "mkfifo"
    );
    refused(&mut peer, render(&fifo, 320, photo_limits()), "FIFO");
    let link = decoder.empty_job();
    symlink("/etc/passwd", link.source()).expect("symlink");
    refused(
        &mut peer,
        render(&link, 320, photo_limits()),
        "symlink to /etc/passwd",
    );
    let hard = decoder.empty_job();
    fs::write(hard.path.join("in/real"), &photo).expect("write the real file");
    fs::hard_link(hard.path.join("in/real"), hard.source()).expect("hard link");
    refused(&mut peer, render(&hard, 320, photo_limits()), "hard link");
    let dir = decoder.empty_job();
    fs::create_dir(dir.source()).expect("directory as source");
    refused(&mut peer, render(&dir, 320, photo_limits()), "directory");

    // `job_dir` as `../x`, as a symlink inside the scratch root and as an absolute path outside it
    let valid = decoder.job(&photo);
    refused(
        &mut peer,
        Request::Probe {
            job_dir: "../x".into(),
        },
        "job_dir ../x",
    );
    let via = decoder.scratch.join("via-link");
    symlink(&valid.path, &via).expect("symlink to a valid job");
    refused(
        &mut peer,
        Request::Probe {
            job_dir: "/scratch/via-link".into(),
        },
        "symlinked job_dir",
    );
    fs::remove_file(&via).expect("remove the symlink");
    refused(
        &mut peer,
        Request::Probe {
            job_dir: "/etc".into(),
        },
        "absolute job_dir outside the scratch root",
    );
    control(&mut peer, &valid);

    assert!(
        decoder.logs().matches("root:x:0:0").next().is_none(),
        "bytes of /etc/passwd in the log"
    );
    for job in [&fifo, &link, &hard, &dir] {
        let outputs = fs::read_dir(job.path.join("out")).expect("out/").count();
        assert_eq!(outputs, 0, "{} has outputs after a refusal", job.id);
    }
    let after = decoder.state();
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    decoder.finish();
}

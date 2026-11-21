//! A snapshot of `/proc`: which processes exist, who their parents and groups are and how much
//! memory they hold.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use crate::sys::page_size;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub pgrp: u32,
    pub rss_bytes: u64,
    /// Ended and waiting to be reaped by its parent.
    pub zombie: bool,
}

/// Every process that exists while the scan runs. One that ends between the directory listing
/// and the read of its `stat` is skipped.
pub fn snapshot() -> io::Result<Vec<Process>> {
    let page = page_size();
    let mut processes = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let Some(stat) = read_stat(&entry.path().join("stat"))? else {
            continue;
        };
        let process = parse_stat(pid, &stat, page).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "unreadable /proc/<pid>/stat")
        })?;
        processes.push(process);
    }
    Ok(processes)
}

/// The text of one `stat` file, or `None` when the process is gone. A process that is reaped
/// between the `open` and the `read` makes the `read` fail with `ESRCH` rather than `ENOENT`,
/// and on a busy machine that window is wide (the scanner is preempted inside it).
fn read_stat(path: &Path) -> io::Result<Option<String>> {
    let mut stat = String::new();
    match File::open(path).and_then(|mut file| file.read_to_string(&mut stat)) {
        Ok(_) => Ok(Some(stat)),
        Err(error) if process_is_gone(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn process_is_gone(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
}

/// The resident memory of every process in group `pgrp`.
pub fn group_rss(pgrp: u32) -> io::Result<u64> {
    Ok(snapshot()?
        .iter()
        .filter(|process| process.pgrp == pgrp)
        .map(|process| process.rss_bytes)
        .sum())
}

/// True while a process of group `pgrp` has not ended (a zombie is ended).
pub fn group_alive(pgrp: u32) -> io::Result<bool> {
    Ok(snapshot()?
        .iter()
        .any(|process| process.pgrp == pgrp && !process.zombie))
}

/// `pid (comm) state ppid pgrp ...`; `comm` may hold spaces and parentheses, so the fields are
/// counted from the last `)`. `rss` is field 24, in pages.
fn parse_stat(pid: u32, stat: &str, page: u64) -> Option<Process> {
    let after_comm = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    Some(Process {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        pgrp: fields.get(2)?.parse().ok()?,
        zombie: *fields.first()? == "Z",
        rss_bytes: fields.get(21)?.parse::<u64>().ok()?.saturating_mul(page),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comm_with_spaces_and_parentheses_does_not_shift_the_fields() {
        let stat = "42 (a) b (c) S 7 9 9 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 100 5000 25 0";

        let process = parse_stat(42, stat, 4096).expect("parsed");

        assert_eq!(
            process,
            Process {
                pid: 42,
                ppid: 7,
                pgrp: 9,
                rss_bytes: 25 * 4096,
                zombie: false,
            }
        );
    }

    #[test]
    fn the_zombie_state_is_read() {
        let stat = "42 (x) Z 7 9 9 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 100 0 0 0";

        assert!(parse_stat(42, stat, 4096).expect("parsed").zombie);
    }

    #[test]
    fn a_process_reaped_between_the_open_and_the_read_is_a_gone_process_not_an_error() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let path = format!("/proc/{}/stat", child.id());
        let mut open = File::open(&path).expect("open stat while the process lives");
        child.kill().expect("kill");
        child.wait().expect("reap");

        let mut text = String::new();
        let raw = open
            .read_to_string(&mut text)
            .expect_err("the kernel refuses the read");
        let scanned = read_stat(Path::new(&path)).expect("a gone process is not an error");

        assert_eq!(raw.raw_os_error(), Some(libc::ESRCH), "{raw}");
        assert_eq!(scanned, None);
    }

    #[test]
    fn a_stat_of_a_live_process_is_read() {
        let text = read_stat(Path::new("/proc/self/stat")).expect("read");

        assert!(
            text.expect("own stat")
                .contains(&std::process::id().to_string())
        );
    }

    #[test]
    fn a_truncated_stat_is_not_parsed() {
        assert_eq!(parse_stat(1, "1 (x) S 0", 4096), None);
    }

    #[test]
    fn the_snapshot_contains_this_process_with_its_own_parent() {
        let me = std::process::id();

        let processes = snapshot().expect("snapshot");

        let mine = processes.iter().find(|p| p.pid == me).expect("self listed");
        assert!(mine.rss_bytes > 0);
        assert!(processes.iter().any(|p| p.pid == mine.ppid));
    }
}

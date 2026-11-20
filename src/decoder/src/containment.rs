//! Containment after every job (10 section 3): the runner is the subreaper of everything its
//! children start, so a descendant that left its process group (`setsid`, a double fork) is
//! still the runner's descendant. After a job every one is killed and reaped, and the next
//! request is accepted only afterwards. A descendant found alive at that point is itself a
//! hostile-class outcome.

use std::io;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::RunnerError;
use crate::proc_table::{Process, snapshot};
use crate::sys::{kill_process, reap_ended_children, set_child_subreaper};

const SWEEP_LIMIT: Duration = Duration::from_secs(3);
const PASS_PAUSE: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sweep {
    /// Nothing but ended processes was left.
    Clean,
    /// At least one live descendant had to be killed.
    KilledLive,
}

/// Makes the runner the parent of every orphaned descendant, so that none escapes the walk.
pub fn become_subreaper() -> Result<(), RunnerError> {
    set_child_subreaper().map_err(RunnerError::io("become a child subreaper"))
}

/// Kills and reaps every descendant of this process, repeating until a full pass finds none.
pub(crate) fn kill_all_descendants() -> io::Result<Sweep> {
    let me = std::process::id();
    let deadline = Instant::now() + SWEEP_LIMIT;
    let mut sweep = Sweep::Clean;
    loop {
        reap_ended_children()?;
        let descendants = descendants_of(me, &snapshot()?);
        if descendants.is_empty() {
            return Ok(sweep);
        }
        for process in descendants.iter().filter(|process| !process.zombie) {
            sweep = Sweep::KilledLive;
            kill_process(process.pid)?;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "descendants of the runner survived the kill-all",
            ));
        }
        thread::sleep(PASS_PAUSE);
    }
}

/// Every process whose chain of parents reaches `root`.
fn descendants_of(root: u32, table: &[Process]) -> Vec<Process> {
    let mut found: Vec<Process> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for process in table.iter().filter(|process| process.ppid == parent) {
            if process.pid != root && !found.iter().any(|known| known.pid == process.pid) {
                found.push(*process);
                frontier.push(process.pid);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ppid: u32) -> Process {
        Process {
            pid,
            ppid,
            pgrp: pid,
            rss_bytes: 0,
            zombie: false,
        }
    }

    #[test]
    fn descendants_are_found_through_any_depth_and_unrelated_processes_are_not() {
        let table = [
            process(1, 0),
            process(10, 1),
            process(100, 10),
            process(101, 100),
            process(102, 100),
            process(200, 1),
            process(300, 200),
        ];

        let mut pids: Vec<u32> = descendants_of(100, &table).iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        let mut from_init: Vec<u32> = descendants_of(1, &table).iter().map(|p| p.pid).collect();
        from_init.sort_unstable();

        assert_eq!(pids, [101, 102]);
        assert_eq!(from_init, [10, 100, 101, 102, 200, 300]);
    }

    #[test]
    fn a_parent_loop_in_the_table_does_not_make_the_walk_run_forever() {
        let table = [process(5, 6), process(6, 5)];

        let found = descendants_of(5, &table);

        assert_eq!(found.iter().map(|p| p.pid).collect::<Vec<_>>(), [6]);
    }
}

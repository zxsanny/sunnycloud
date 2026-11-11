# Leftover: AZ-3722 group-commit filesystem spike needs a privileged runner

kind: blocker
blocked action: measure 400 writers of 2 MiB blocks for 3 minutes on a loop-mounted XFS image and a loop-mounted ext4 image of equal size (AZ-3722 AC-1)
reason: this host has `mkfs.xfs`, `mkfs.ext4` and `losetup` (24 cores, kernel 7.0.0-42), but `sudo -n true` answers "interactive authentication is required", so no image can be mounted. The spec forbids a substitute (tmpfs or another filesystem), so nothing was measured and the task stays in `todo/` (Jira `To Do`).

Unblock (one of):
1. Passwordless sudo for `mount`, `umount` and `losetup` on this machine (the probe then mounts two 20 GiB images under `/tmp` and unmounts them at the end), or
2. Two empty, already mounted directories, one XFS and one ext4, on the same physical disk, handed to the agent (paths in the next message), or
3. Run on the reference server once it exists (REF-SRV, R03); numbers from this host stay informational either way.

Replay: build the throwaway probe outside the repo (pwrite 2 MiB, one group `fdatasync` per dirty file, waiters released after the sync), run 3 minutes per filesystem, record p50/p95/p99 ack wait, flushes per second and MB/s with the host's CPU, disk and kernel, then write the conclusion into the blobstore description Risks row "Group commit stalls on a slow disk" (AC-3). No probe code is merged (AC-4). Delete this file when the spike is done.

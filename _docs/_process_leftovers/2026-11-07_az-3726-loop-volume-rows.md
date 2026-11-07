# Leftover: AZ-3726 privileged loop-volume rows (NOT-RUN(C-priv))

kind: test-run
blocked action: run the four AZ-3726 rows that need the loop-volume tool and a privileged runner
reason: no privileged runner on the workstation (`.claude/rules/testing.md`: higher tiers run on their designated host; a workstation substitute is not a run)

Rows (spec `_docs/02_tasks/done/AZ-3726_blob_writer.md`):
- AC-3: 64 MiB volume, 8 MiB reserve, 60 MiB create with a filler file -> `FreeSpaceFloor`, no file left
- AC-3: a preallocated blob keeps writing on a full disk
- AC-4: full volume where the operating system returns `ENOSPC` mid-write -> `NoSpace`, file removed
- IT-04-7 probe

The spec says AZ-3726 is not done until these run. Jira stays at In Testing, not Done, until a privileged runner executes them. Replay: run the writer tests with the loop-volume tool on the designated runner and record the result in the next batch report.

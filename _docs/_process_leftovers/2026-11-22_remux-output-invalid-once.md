# Remux test: one OutputInvalid in a parallel run, not reproduced

Observed once while building AZ-3860: `a_remux_copies_the_packets_and_moves_the_index_to_the_front` (src/decoder/tests/video_transcode.rs)
answered `OutputInvalid` in the 583-test parallel run on a host at a load average above 30. It did not recur in 6 isolated
runs, 7 full-binary runs and 3 final runs of the `video_transcode` and `video_kills` binaries (worker), nor in 40 isolated
runs on 2026-11-22 (parent). The test now prints the runner log when a result is missing, so a second occurrence shows the
cause (the runner's verdict is `OutputInvalid` when the output is not H.264 in MP4 or its duration differs from the input by
more than 1 s).

Do not retry or loosen: on a second occurrence (CI or local) read the printed runner log and the ffprobe of the output.
A spurious permanent `OutputInvalid` in production would be a defect, not a test problem.

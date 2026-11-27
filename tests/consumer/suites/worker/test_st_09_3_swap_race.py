"""ST-09-3 (AC-25) on H2: decoder output cannot exfiltrate files. FD plays a compromised decoder: each render leaves a
1x1 WebP at `out/result.webp` and a symlink beside it, to the worker secret or, every other render, to the same file
through the reader's `/proc/self/root` (the `/proc/self` tricks of the attack vector). Once the answer is sent FD exchanges the two names
10,000 times (atomic rename-exchange) while the supervisor opens and reads the output. Over 100 photo jobs every posted
body is the WebP's bytes or nothing: a job whose render met the symlink ends `dead` with `output_rejected`, one whose
renders met the file stores the WebP. Both outcomes must occur, or the swaps did not race the supervisor's open. The
other hostile files (symlink, FIFO, sparse, directory, hard link) are IT-09-12's.

Posted bodies are seen from the API: every answer on the output route is 2xx (the API's sniffer refuses a body that
is not WebP, such as the secret), and every derived file of the 100 photos that the routes serve reads back as the WebP."""
import re
import time

import mediakit
from h2cases import (OUTPUT_ROUTE, PHOTO, POLL_SECONDS, WEBP_1X1, api_requests, chunks, closed_rows, fd_on_general,
                     job_rows, photo_probe)

RUNS = 100
SWAPS = 10_000
RUN_SECONDS = 300
SETTLE_SECONDS = 10
TARGETS = ("/run/secrets/worker_api_token", "/proc/self/root/run/secrets/worker_api_token")
EXCHANGED = re.compile(r"^exchanged (\d+) of (\d+)")


def swap_render(target):
    return {"image": {"width": 1, "height": 1,
                      "output": {"swap": {"target": target, "bytes": chunks(WEBP_1X1), "times": SWAPS}}}}


# At most two renders per job: the targets alternate through every render the run can ask for.
SCRIPT = {"probe": {"default": {"probe": photo_probe()}},
          "render_still": {"queue": [swap_render(TARGETS[i % 2]) for i in range(2 * RUNS)],
                           "default": swap_render(TARGETS[0])}}


def settled(read, done, seconds=SETTLE_SECONDS):
    """`read()` once `done(value)` holds, or the last value after `seconds`."""
    deadline = time.monotonic() + seconds
    while True:
        value = read()
        if done(value) or time.monotonic() > deadline:
            return value
        time.sleep(POLL_SECONDS)


def test_st_09_3_a_swapped_output_name_never_posts_anything_but_the_regular_files_bytes(stack_ctl, media, lanes):
    # Arrange
    fd = fd_on_general(lanes, SCRIPT)
    rows_before = job_rows(stack_ctl)
    posted_before = api_requests(stack_ctl, OUTPUT_ROUTE, "PUT")

    # Act
    photos = [media.commit(f"st-09-3-{run}.jpg", mediakit.with_marker(media.fixture(PHOTO), f"st-09-3-{run}"))
              for run in range(RUNS)]
    rows = closed_rows(stack_ctl, rows_before, RUNS, RUN_SECONDS)
    posted = api_requests(stack_ctl, OUTPUT_ROUTE, "PUT")
    # The last render's `swap_end` follows the job's close: the exchanges end when the supervisor removes the job tree.
    record = settled(fd.record, lambda lines: sum(line["kind"] == "swap_end" for line in lines)
                     == sum(line["kind"] == "render_still" for line in lines))
    scratch = settled(lambda: stack_ctl.call("inspect", scratch="general")["entries"], lambda entries: entries == [])
    derived = [media.derived_file(photo["node_id"], f"thumbnail?size={size}")
               for photo in photos for size in ("preview", "small")]

    # Assert
    outcomes = sorted({(row["kind"], row["status"], row["last_error"]) for row in rows.values()})
    split = {status: sum(row["status"] == status for row in rows.values()) for status in ("succeeded", "dead")}
    print(f"ST-09-3: {split} of {RUNS} runs")
    assert len(rows) == RUNS, f"{len(rows)} of {RUNS} jobs closed"
    assert set(outcomes) <= {("stills", "succeeded", ""), ("stills", "dead", "output_rejected")}, outcomes
    # The race is real only if the supervisor's open met both names: a run that saw the symlink ends `dead`.
    assert split["dead"] > 0 and split["succeeded"] > 0, f"the swaps did not race the supervisor's open: {split}"
    answers = {status: count - posted_before.get(status, 0) for status, count in posted.items()}
    assert {status: n for status, n in answers.items() if n and status != "2xx"} == {}, answers
    served = [body for status, _, body in derived if status == 200]
    assert len(served) >= 2 * split["succeeded"], (len(served), split)
    assert [body for body in served if body != WEBP_1X1] == []
    ends = [line for line in record if line["kind"] == "swap_end"]
    assert not [line for line in record if line["kind"] == "fd_error"], "FD could not plant an output"
    assert len(ends) == len([line for line in record if line["kind"] == "render_still"]), "a render was not raced"
    exchanged = [EXCHANGED.match(line["detail"]) for line in ends]
    assert all(match and int(match.group(1)) > 0 for match in exchanged), [line["detail"] for line in ends][:5]
    assert scratch == [], scratch

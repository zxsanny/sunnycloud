"""IT-09-2 (AC-10) and IT-09-4 (AC-11) on H2 with the real decoders: the supervisor's side of a photo job and of the
transcode decisions on real clips. The media suite (AZ-3661, harness core) checks the files themselves; these cases
check what the supervisor did: one input fetch for both renders, the preview posted before the thumbnail, the job
metrics, and for clips no transcode post when the clip is compliant and a remux that keeps every coded packet.

Not on the stack: the render requests themselves (the real decoder logs no request; their sizes show in the derived
files: 1440 and 320 on the long edge) and the heartbeats of a job that ends within one heartbeat period (the cadence is
IT-09-10's, on FD). The packet hash is read from the sample tables of both files (`mediakit.track_sample_hashes`, which
gives ffmpeg's own packet SHA-256), as the consumer image carries no media tools."""
import pytest

import mediakit
from h2cases import (INPUT_ROUTE, OUTPUT_ROUTE, PHOTO, api_requests, closed_rows, derived_order, job_rows,
                     worker_jobs_total)
from jobkit import JOBS_DONE_SECONDS

COMPLIANT = ("clip-compliant-front.mp4", "clip-compliant-portrait.mp4")
INDEX_AT_THE_END = "clip-compliant-end.mp4"


def fetches(ctl):
    """Every answer on the input route, whatever its status: a second fetch that failed is still a second fetch."""
    return sum(api_requests(ctl, INPUT_ROUTE, "GET").values())


def posts(ctl):
    return sum(api_requests(ctl, OUTPUT_ROUTE, "PUT").values())


def closed(ctl, rows_before, jobs):
    return closed_rows(ctl, rows_before, jobs, JOBS_DONE_SECONDS)


def test_it_09_2_a_photo_job_fetches_its_input_once_and_posts_the_preview_then_the_thumbnail(stack_ctl, media):
    # Arrange
    rows_before = job_rows(stack_ctl)
    fetched_before, ok_before = fetches(stack_ctl), worker_jobs_total(stack_ctl, "stills", "ok")

    # Act
    photo = media.commit("it-09-2.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-2"))
    rows = closed(stack_ctl, rows_before, 1)
    preview = media.derived_file(photo["node_id"], "thumbnail?size=preview")
    small = media.derived_file(photo["node_id"], "thumbnail?size=small")
    scratch = {lane: stack_ctl.call("inspect", scratch=lane)["entries"] for lane in ("interactive", "general")}

    # Assert
    assert [(r["kind"], r["status"], r["attempts"]) for r in rows.values()] == [("stills", "succeeded", 1)], rows
    assert fetches(stack_ctl) - fetched_before == 1, "one input fetch serves both renders"
    assert derived_order(stack_ctl, photo["content_hash"]) == ["preview", "thumb"]
    # Stored 2400x1800, shown rotated: the long edges are the renders' 1440 and 320.
    assert (mediakit.webp_size(preview[2]), mediakit.webp_size(small[2])) == ((1080, 1440), (240, 320))
    assert scratch == {"interactive": [], "general": []}
    assert worker_jobs_total(stack_ctl, "stills", "ok") - ok_before == 1


@pytest.mark.parametrize("fixture", COMPLIANT, ids=["landscape", "portrait"])
def test_it_09_4_a_compliant_clip_is_skipped_without_a_post(stack_ctl, media, fixture):
    # Arrange
    rows_before, posted_before = job_rows(stack_ctl), posts(stack_ctl)
    skipped_before = stack_ctl.call("db-probe", query="media_jobs")["skipped_compliant"]

    # Act
    clip = media.commit(f"it-09-4-{fixture}", mediakit.with_marker(media.fixture(fixture), f"it-09-4-{fixture}"))
    rows = closed(stack_ctl, rows_before, 2)
    kinds = derived_order(stack_ctl, clip["content_hash"])

    # Assert: the poster posts its thumbnail; the transcode closes `skipped_compliant` and posts nothing.
    transcode = [r for r in rows.values() if r["kind"] == "transcode"]
    assert [(r["status"], r["last_error"]) for r in transcode] == [("succeeded", "")], rows
    assert stack_ctl.call("db-probe", query="media_jobs")["skipped_compliant"] - skipped_before == 1
    assert posts(stack_ctl) - posted_before == 1, "only the poster's thumbnail is posted"
    assert kinds == ["thumb"], kinds
    assert media.derived_file(clip["node_id"], "transcode")[0] == 404


def test_it_09_4_a_clip_with_its_index_at_the_end_is_remuxed_keeping_every_coded_packet(stack_ctl, media):
    # Arrange
    rows_before = job_rows(stack_ctl)
    remuxed_before = worker_jobs_total(stack_ctl, "transcode", "remuxed")
    original = mediakit.with_marker(media.fixture(INDEX_AT_THE_END), "it-09-4-end")

    # Act
    clip = media.commit("it-09-4-end.mp4", original)
    rows = closed(stack_ctl, rows_before, 2)
    status, _, remuxed = media.derived_file(clip["node_id"], "transcode")

    # Assert
    assert all(r["status"] == "succeeded" for r in rows.values()), rows
    assert status == 200
    assert derived_order(stack_ctl, clip["content_hash"]).count("transcode") == 1
    assert (mediakit.mp4_facts(original)["moov_first"], mediakit.mp4_facts(remuxed)["moov_first"]) == (False, True)
    tracks = mediakit.track_sample_hashes(original)
    assert set(tracks) == {"vide", "soun"}, tracks
    assert mediakit.track_sample_hashes(remuxed) == tracks
    assert worker_jobs_total(stack_ctl, "transcode", "remuxed") - remuxed_before == 1

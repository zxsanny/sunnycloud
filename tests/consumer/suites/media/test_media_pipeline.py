"""AZ-3661 (flow F10, second half) on the stack: committed photos and clips become the expected derived files through
the real api, the real supervisor and the real decoder containers, and the real routes serve them. No fake decoder.

One account commits a photo with EXIF orientation 6, three clips and two hostile files; the module waits until the
supervisor has closed every job, then each test reads its own file's outcome. Fixtures: `tests/tools/gen_media.py`.
"""
import pytest

import mediakit
from e2e_report import skip_not_run

HOSTILE_SETTLE_SECONDS = 10


@pytest.fixture(scope="module")
def committed(media):
    """Every fixture committed, then every job closed: photo (1 job), three clips (poster + transcode each), a truncated
    HEIC (1 job). The SVG named .png gets no job (the planner never sends an SVG to a decoder)."""
    files = {name: media.commit(name) for name in (
        "photo-orientation6.jpg", "clip-noncompliant.mp4", "clip-compliant-front.mp4", "clip-compliant-end.mp4",
        "hostile-svg.png", "truncated.heic")}
    jobs = media.wait_jobs_closed(closed=8)
    return files, jobs


def test_photo_with_orientation_6_gets_a_swapped_thumbnail_and_preview(media, committed):
    # Arrange
    files, _ = committed
    photo = files["photo-orientation6.jpg"]

    # Act
    state = media.wait_state(photo["content_hash"], "ready")
    small = media.derived_file(photo["node_id"], "thumbnail?size=small")
    preview = media.derived_file(photo["node_id"], "thumbnail?size=preview")

    # Assert: stored 2400x1800, shown rotated, so portrait: 240x320 and 1080x1440
    assert state == "ready"
    assert (small[0], small[1]["Content-Type"]) == (200, "image/webp")
    assert (preview[0], preview[1]["Content-Type"]) == (200, "image/webp")
    assert (mediakit.webp_size(small[2]), mediakit.webp_size(preview[2])) == ((240, 320), (1080, 1440))


def test_clip_above_the_bitrate_limit_is_transcoded_with_a_poster(media, committed):
    # Arrange
    files, _ = committed
    clip = files["clip-noncompliant.mp4"]

    # Act
    transcode = media.derived_file(clip["node_id"], "transcode")
    poster = media.derived_file(clip["node_id"], "thumbnail?size=small")

    # Assert
    assert media.state(clip["content_hash"]) == "ready"
    assert (transcode[0], transcode[1]["Content-Type"]) == (200, "video/mp4")
    facts = mediakit.mp4_facts(transcode[2])
    assert (facts["h264"], facts["profile_idc"], facts["aac"], facts["moov_first"]) == (True, 100, True, True)
    assert max(facts["width"], facts["height"]) <= 1920 and min(facts["width"], facts["height"]) <= 1080
    assert len(transcode[2]) < len(media.fixture("clip-noncompliant.mp4")), "the transcode is under the bitrate limit, so smaller"
    assert (poster[0], poster[1]["Content-Type"]) == (200, "image/webp")


def test_compliant_clip_with_the_index_first_is_skipped_and_gets_no_transcode(media, committed):
    # Arrange
    files, jobs = committed
    clip = files["clip-compliant-front.mp4"]

    # Act
    transcode = media.derived_file(clip["node_id"], "transcode")
    poster = media.derived_file(clip["node_id"], "thumbnail?size=small")

    # Assert
    assert jobs["skipped_compliant"] == 1, jobs
    assert transcode[0] == 404, "no transcode exists for a clip that needs none"
    assert poster[0] == 200


def test_compliant_clip_with_the_index_last_is_remuxed_not_reencoded(media, committed):
    # Arrange
    files, _ = committed
    clip = files["clip-compliant-end.mp4"]
    original = media.fixture("clip-compliant-end.mp4")

    # Act
    transcode = media.derived_file(clip["node_id"], "transcode")

    # Assert: same picture and streams, index moved to the front, the coded data is the same size (a re-encode is not)
    assert transcode[0] == 200
    before, after = mediakit.mp4_facts(original), mediakit.mp4_facts(transcode[2])
    assert (before["moov_first"], after["moov_first"]) == (False, True)
    assert (after["width"], after["height"], after["profile_idc"]) == (before["width"], before["height"], before["profile_idc"])
    assert mediakit.mdat_size(transcode[2]) == mediakit.mdat_size(original)


def test_truncated_heic_fails_its_job_and_the_original_stays_downloadable(media, committed):
    # Arrange
    files, jobs = committed
    heic = files["truncated.heic"]

    # Act
    state = media.state(heic["content_hash"])
    original = media.browser.fetch(f"/api/v1/nodes/{heic['node_id']}/content")
    thumbnail = media.derived_file(heic["node_id"], "thumbnail?size=small")

    # Assert
    assert (jobs["dead"], state) == (1, "failed")
    assert (original[0], original[2]) == (200, media.fixture("truncated.heic"))
    assert thumbnail[0] == 404


def test_svg_named_png_is_never_decoded_and_the_original_stays_downloadable(media, committed):
    # Arrange
    files, jobs = committed
    svg = files["hostile-svg.png"]

    # Act
    original = media.browser.fetch(f"/api/v1/nodes/{svg['node_id']}/content")
    thumbnail = media.derived_file(svg["node_id"], "thumbnail?size=small")

    # Assert: the content is sniffed, an SVG is not an image type the planner knows, so there is no job at all
    assert media.state(svg["content_hash"]) is None
    assert (original[0], original[2]) == (200, media.fixture("hostile-svg.png"))
    assert thumbnail[0] == 404
    assert jobs["dead"] == 1, "only the truncated HEIC failed"


def test_a_corrupt_file_does_not_restart_the_decoder_and_the_next_photo_still_works(stack_ctl, media, committed):
    # Arrange: the HEIC job is closed; both decoder containers have been up since the stack started
    general, interactive = stack_ctl.call("inspect", service="decoder-general"), stack_ctl.call("inspect", service="decoder-interactive")

    # Act
    photo = media.commit("photo-orientation6.jpg", mediakit.with_marker(media.fixture("photo-orientation6.jpg"), "after-corrupt"))
    state = media.wait_state(photo["content_hash"], "ready")

    # Assert
    assert state == "ready"
    for before, service in ((general, "decoder-general"), (interactive, "decoder-interactive")):
        after = stack_ctl.call("inspect", service=service)
        assert (after["started_at"], after["restart_count"], after["status"]) == (before["started_at"], 0, "running"), service


def test_a_hostile_class_decoder_exit_is_restarted_by_compose():
    skip_not_run("NO-SERVICE", "no committed file makes the decoder exit as hostile-class (a corrupt file is a clean Corrupt, "
                               "see the test above); that leg needs the decoder test image with test_freeze (decoder suite IT-10-14/15)")


def test_doctor_reports_the_worker_decoder_and_image_age(stack_ctl, media, committed):
    # Arrange: the supervisor reports every worker_status_interval (30 s)
    import time

    deadline = time.monotonic() + 45
    while stack_ctl.call("db-probe", query="worker_status") != {"workers": 1, "decoder_ok": 1} and time.monotonic() < deadline:
        time.sleep(2)

    # Act
    result = stack_ctl.call("exec-operator", args=["doctor"])

    # Assert
    line = next(l for l in result["stdout"].splitlines() if "check: media_worker" in l)
    assert "level: ok" in line, line

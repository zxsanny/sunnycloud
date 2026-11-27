"""AT-09-1 (AC-11) on H2 with the real decoders: the component acceptance of the transcode policy, from the user's
side. A compliant clip (DS-07) closes `skipped_compliant`: no transcode is served and no transcode bytes are stored. A
clip with its index at the end (DS-35) gets a transcode that is a remux: index first, the same coded streams as the
original. IT-09-4 checks the supervisor's side of the same decisions (posts, metrics)."""
import mediakit
from h2cases import derived_order, terminal
from jobkit import JOBS_DONE_SECONDS

COMPLIANT = "clip-compliant-front.mp4"
INDEX_AT_THE_END = "clip-compliant-end.mp4"
JOBS_PER_CLIP = 2  # the poster and the transcode


def upload_and_wait(media, name, data):
    """Commits a clip and waits until its two jobs are closed; the job counts before and after."""
    before = media.jobs()
    clip = media.commit(name, data)
    return clip, before, media.wait_jobs_closed(terminal(before) + JOBS_PER_CLIP, JOBS_DONE_SECONDS)


def test_at_09_1_a_compliant_clip_leaves_no_output_and_a_clip_with_its_index_at_the_end_is_remuxed(stack_ctl, media):
    # Arrange
    original = mediakit.with_marker(media.fixture(INDEX_AT_THE_END), "at-09-1-end")

    # Act
    compliant, before, after = upload_and_wait(media, "at-09-1-compliant.mp4",
                                               mediakit.with_marker(media.fixture(COMPLIANT), "at-09-1-compliant"))
    compliant_transcode = media.derived_file(compliant["node_id"], "transcode")
    end, _, _ = upload_and_wait(media, "at-09-1-end.mp4", original)
    status, _, remuxed = media.derived_file(end["node_id"], "transcode")

    # Assert: step 1, the compliant clip.
    assert after["skipped_compliant"] - before["skipped_compliant"] == 1, (before, after)
    assert compliant_transcode[0] == 404
    assert "transcode" not in derived_order(stack_ctl, compliant["content_hash"])
    # Step 2, the clip with its index at the end.
    assert status == 200
    assert derived_order(stack_ctl, end["content_hash"]).count("transcode") == 1
    assert (mediakit.mp4_facts(original)["moov_first"], mediakit.mp4_facts(remuxed)["moov_first"]) == (False, True)
    tracks = mediakit.track_sample_hashes(original)
    assert set(tracks) == {"vide", "soun"}, tracks
    assert mediakit.track_sample_hashes(remuxed) == tracks

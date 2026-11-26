"""IT-09-3 (AC-11, AC-10) and IT-09-5 (AC-44, AC-11) on H2: the supervisor computes the poster offset and applies the
transcode policy to the probe FD reports, and a non-compliant decoder output fails the job once, without a post.

Every job of a clip goes through FD on the general lane: the interactive slot is gated out, so it claims nothing. FD's
probes are scripted, so these cases check decisions and protocol traffic, never a media outcome."""

from h2cases import run_file, video_probe


def script(probe, transcode=None):
    """Both jobs of a clip (poster and transcode) probe first, so the probe is queued twice."""
    return {"probe": {"queue": [{"probe": probe}, {"probe": probe}], "default": {"error": "unsupported"}},
            "render_still": {"default": {"image": {"width": 320, "height": 180}}},
            "transcode": {"default": transcode or {"error": "corrupt"}}}


def run_clip(stack_ctl, media, lanes, name, fd_script):
    _, record, before, after = run_file(media, lanes, name, fd_script)
    return record, before, after


def requests_of(record, kind):
    return [line["fields"] for line in record if line["kind"] == kind]


def test_it_09_3_the_poster_of_a_9_second_clip_is_at_900_ms_and_a_compliant_clip_is_skipped(stack_ctl, media, lanes):
    # Act
    record, before, after = run_clip(stack_ctl, media, lanes, "it-09-3-nine", script(video_probe(duration_ms=9_000)))

    # Assert
    assert [r["poster_at_ms"] for r in requests_of(record, "render_still")] == [900], record
    assert requests_of(record, "transcode") == [], record
    assert after["skipped_compliant"] - before["skipped_compliant"] == 1, (before, after)


def test_it_09_3_the_poster_of_a_60_second_clip_is_at_1000_ms(stack_ctl, media, lanes):
    # Act
    record, _, _ = run_clip(stack_ctl, media, lanes, "it-09-3-sixty", script(video_probe(duration_ms=60_000)))

    # Assert
    assert [r["poster_at_ms"] for r in requests_of(record, "render_still")] == [1000], record


def test_it_09_3_a_compliant_portrait_clip_gets_no_transcode(stack_ctl, media, lanes):
    # Act
    record, before, after = run_clip(stack_ctl, media, lanes, "it-09-3-portrait",
                                     script(video_probe(width=1080, height=1920)))

    # Assert
    assert requests_of(record, "transcode") == [], record
    assert after["skipped_compliant"] - before["skipped_compliant"] == 1, (before, after)


def test_it_09_3_an_index_at_the_end_is_remuxed(stack_ctl, media, lanes):
    # Act
    record, _, _ = run_clip(stack_ctl, media, lanes, "it-09-3-moov-end", script(video_probe(moov_first=False)))

    # Assert
    assert [r["plan"] for r in requests_of(record, "transcode")] == ["remux"], record


def test_it_09_3_a_bitrate_of_10_5_mbit_is_transcoded_on_the_cpu_with_an_enum_only_plan(stack_ctl, media, lanes):
    # Act
    record, _, _ = run_clip(stack_ctl, media, lanes, "it-09-3-bitrate", script(video_probe(bitrate=10_500_000)))

    # Assert
    plans = [r["plan"] for r in requests_of(record, "transcode")]
    assert len(plans) == 1 and set(plans[0]) == {"transcode"}, record
    plan = plans[0]["transcode"]
    assert (plan["encoder"], plan["faststart"], plan["tone_map"]) == ("cpu", True, False), plan
    assert all(isinstance(value, (bool, int)) or value in ("cpu", "nvenc", "aac", "none") for value in plan.values()), plan


def test_it_09_5_a_non_compliant_decoder_output_fails_the_job_once_without_a_post(stack_ctl, media, lanes):
    # Arrange: the input needs a transcode, and FD's output is still far over the bitrate bound.
    output = {"transcode": {"probe_of_output": video_probe(bitrate=12_000_000), "output": "file"}}
    fd_script = script(video_probe(bitrate=12_000_000), transcode=output)
    derived_before = stack_ctl.call("db-probe", query="derived_rows")

    # Act
    record, before, after = run_clip(stack_ctl, media, lanes, "it-09-5-output", fd_script)
    attempts = stack_ctl.call("db-probe", query="job_attempts")

    # Assert
    assert len(requests_of(record, "transcode")) == 1, record
    assert after["dead"] - before["dead"] >= 1, (before, after)
    assert stack_ctl.call("db-probe", query="derived_rows")["transcode"] == derived_before["transcode"]
    assert attempts["most"] == 1, attempts

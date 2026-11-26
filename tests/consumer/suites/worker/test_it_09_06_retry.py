"""IT-09-6 (AC-45) on H2: the one rerun inside a claim (after `Stalled` or `EncoderFailed`), and the disposition of every
other decoder failure, with FD scripting the decoder on the general lane.

Leg (i), a fetch that fails on the server, is AZ-4387's (decision D2). Leg (c) needs NVENC, which the v1.0 stack never
enables (`nvenc = off`; the CPU rerun after an NVENC failure is `src/worker/tests/slot_loop_nvenc.rs`)."""
import time

import pytest

from e2e_report import skip_not_run
from h2cases import PHOTO, photo_probe, postable_image, postable_transcode, requests_of, run_file, video_probe

RETRY_SECONDS = 60
POLL_SECONDS = 0.25
NEEDS_TRANSCODE = video_probe(bitrate=12_000_000)


def clip_script(probe, transcodes, fallback=None):
    """Both jobs of a clip probe first; the poster is posted, the transcode answers come in order."""
    return {"probe": {"queue": [{"probe": probe}, {"probe": probe}], "default": {"error": "unsupported"}},
            "render_still": {"default": postable_image()},
            "transcode": {"queue": transcodes, "default": fallback or {"error": "corrupt"}}}


def photo_script(probe_answer):
    return {"probe": {"default": probe_answer}, "render_still": {"default": postable_image()}}


def transcodes(record):
    return requests_of(record, "transcode")


def test_it_09_6_a_stalled_transcode_is_rerun_once_on_the_cpu_into_an_empty_out_and_the_job_succeeds(media, lanes):
    # Act
    _, record, before, after = run_file(media, lanes, "it-09-6-a", clip_script(
        NEEDS_TRANSCODE, [{"error": "stalled"}, postable_transcode()]),
        wanted=lambda before, after: after["derived"] - before["derived"] >= 2)

    # Assert
    calls = transcodes(record)
    assert len(calls) == 2, record
    assert calls[1]["out_entries"] == 0
    assert calls[1]["fields"]["plan"]["transcode"]["encoder"] == "cpu"
    assert after["derived"] - before["derived"] == 2, (before, after)


def test_it_09_6_b_a_second_stall_is_permanent_and_the_job_is_dead(media, lanes):
    # Act
    _, record, before, after = run_file(media, lanes, "it-09-6-b", clip_script(
        NEEDS_TRANSCODE, [{"error": "stalled"}, {"error": "stalled"}]))

    # Assert
    assert len(transcodes(record)) == 2, record
    assert after["dead"] - before["dead"] == 1, (before, after)


def test_it_09_6_c_an_nvenc_failure_is_rerun_on_the_cpu():
    skip_not_run("NO-SERVICE", "the v1.0 stack runs nvenc = off, so no plan names NVENC; covered by "
                               "src/worker/tests/slot_loop_nvenc.rs (H-GPU, v1.1)")


def test_it_09_6_d_a_failed_remux_is_rerun_as_a_cpu_transcode(media, lanes):
    # Act
    _, record, before, after = run_file(media, lanes, "it-09-6-d", clip_script(
        video_probe(moov_first=False), [{"error": {"encoder_failed": {"exit_code": 1}}}, postable_transcode()]),
        wanted=lambda before, after: after["derived"] - before["derived"] >= 2)

    # Assert
    plans = [line["fields"]["plan"] for line in transcodes(record)]
    assert plans[0] == "remux" and len(plans) == 2, record
    assert set(plans[1]) == {"transcode"} and plans[1]["transcode"]["encoder"] == "cpu", plans


@pytest.mark.parametrize("answer", ["unsupported", "corrupt", {"limit_exceeded": {"limit": "wall"}}],
                         ids=["e-unsupported", "f-corrupt", "g-limit-exceeded"])
def test_it_09_6_e_to_g_a_final_decoder_answer_kills_the_job_at_its_first_attempt(stack_ctl, media, lanes, answer):
    # Act
    name = f"it-09-6-{answer if isinstance(answer, str) else 'limit'}"
    _, record, before, after = run_file(media, lanes, name, photo_script({"error": answer}), fixture=PHOTO, ended=1)
    attempts = stack_ctl.call("db-probe", query="job_attempts")

    # Assert
    assert len(requests_of(record, "probe")) == 1, record
    assert after["dead"] - before["dead"] == 1, (before, after)
    assert attempts["most"] == 1, attempts


def test_it_09_6_h_a_socket_closed_mid_job_fails_retry_and_the_job_comes_back_after_its_backoff(stack_ctl, media, lanes):
    # Act: every probe of the photo hangs up, so every attempt ends with a transient failure.
    fd, _, _, _ = run_file(media, lanes, "it-09-6-h", photo_script("hang"), fixture=PHOTO,
                           wanted=lambda before, after: True)
    deadline = time.monotonic() + RETRY_SECONDS
    while len(requests_of(fd.record(), "probe")) < 2 and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    probes = requests_of(fd.record(), "probe")
    attempts = stack_ctl.call("db-probe", query="job_attempts")

    # Assert
    assert len(probes) >= 2, fd.record()
    assert probes[1]["at_ms"] - probes[0]["at_ms"] >= 1000, probes
    assert probes[1]["job"] == probes[0]["job"] and probes[1]["conn"] != probes[0]["conn"], probes
    assert attempts["most"] >= 2, attempts


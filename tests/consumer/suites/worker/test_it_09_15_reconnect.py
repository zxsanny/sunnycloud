"""IT-09-15 (AC-25) on H2: after the decoder runner exits while the slot is idle, the supervisor notices it before its
next claim, reconnects, and repeats `hello` and `self_check` before it claims again; the outage counts one restart."""
import mediakit
from fdkit import FAILED_SELF_CHECK, JOB_REQUESTS

GATE_SECONDS = 30
NOTICE_SECONDS = 10
# The reconnect backoff doubles from 1 s up to 30 s; a short outage is over within a few attempts.
RECONNECT_SECONDS = 60


def restarts(ctl, lane):
    return ctl.worker_metrics().get(f'sunnycloud_worker_decoder_restarts_total{{slot="{lane}"}}', 0.0)


def test_it_09_15_a_real_runner_killed_while_idle_is_noticed_gated_again_and_counted_once(stack_ctl):
    # Arrange
    assert stack_ctl.wait_worker_readyz(200, GATE_SECONDS) == 200
    before = restarts(stack_ctl, "interactive")

    # Act
    stack_ctl.kill_decoder("decoder-interactive")
    try:
        noticed = stack_ctl.wait_worker_readyz(503, NOTICE_SECONDS)
    finally:
        stack_ctl.start_decoder("decoder-interactive")
    back = stack_ctl.wait_worker_readyz(200, RECONNECT_SECONDS)
    after = restarts(stack_ctl, "interactive")

    # Assert
    assert noticed == 503, "the supervisor did not notice that its idle decoder went away"
    assert back == 200
    assert after - before == 1.0


def test_it_09_15_after_an_outage_the_first_requests_are_hello_then_self_check_then_the_job(stack_ctl, media, lanes):
    # Arrange: the general slot is gated out, so only the interactive slot can claim the photo.
    lanes("general", {"self_check": FAILED_SELF_CHECK}, restart_worker=False)
    fd = lanes("interactive", {"idle_outage": {"after_ms": 500, "refuse_ms": 3000}})
    fd.wait_for(["hello", "self_check", "outage_start"], GATE_SECONDS)

    # Act
    media.commit("it-09-15.jpg", mediakit.with_marker(media.fixture("photo-orientation6.jpg"), "it-09-15"))
    record = fd.wait_for(["outage_start", "outage_end", "hello", "self_check", "probe"], RECONNECT_SECONDS)

    # Assert
    first_conn = record[0]["conn"]
    jobs_before_outage = [line for line in record if line["conn"] == first_conn and line["kind"] in JOB_REQUESTS]
    after_outage = [line["kind"] for line in record if line["conn"] not in (0, first_conn)]
    assert jobs_before_outage == [], record
    assert after_outage[:3] == ["hello", "self_check", "probe"], record

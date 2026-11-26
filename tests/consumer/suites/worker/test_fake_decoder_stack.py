"""FD on the real decoder socket of the stack (AZ-4382 AC-3): with FD in place of the interactive decoder, the real
supervisor starts, completes `hello` and `self_check` with it, the record shows both, and the real decoder comes back
after (fdkit restarts the supervisor on each swap).

Harness H2 only (`tests/run.sh --profile h2`): the `core` stack has no FD service."""
from fdkit import FakeDecoderLane

GATE_SECONDS = 30
WORKER_READY_SECONDS = 90


def test_the_supervisor_completes_hello_and_self_check_with_fd_on_the_interactive_socket(stack_ctl):
    # Arrange
    assert stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS) == 200
    fd = FakeDecoderLane(stack_ctl, "interactive")

    # Act
    fd.start({"self_check": [{"name": "no_network", "ok": True}]})
    try:
        record = fd.wait_for(["hello", "self_check"], GATE_SECONDS)
        ready_on_fd = stack_ctl.wait_worker_readyz(200, GATE_SECONDS)
    finally:
        fd.stop()
    ready_on_rd = stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS)

    # Assert
    first = [line for line in record if line["conn"] == record[0]["conn"]]
    assert [line["kind"] for line in first[:2]] == ["hello", "self_check"], record
    assert first[0]["fields"]["protocol"] >= 1
    assert not [line for line in record if line["kind"] in ("invalid", "fd_error")], record
    assert (ready_on_fd, ready_on_rd) == (200, 200)

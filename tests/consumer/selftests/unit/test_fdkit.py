"""Unit tests of fdkit, the suites' side of the fake decoder: argument checks, the script and record files, and the
in-order match the waits use. No stack needed: the ctl transport is a recorder."""
import json

import pytest

import fdkit


class RecordingCtl:
    def __init__(self):
        self.calls = []

    def call(self, action, **params):
        self.calls.append((action, params))
        return {}

    def try_call(self, action, **params):
        self.calls.append((action, params))
        return 200, {"result": {"log_tail": "fake-decoder: cannot read the script: Permission denied"}}

    def restart_worker(self):
        self.call("process-kill", service="worker", op="kill")
        self.call("process-kill", service="worker", op="start")


@pytest.fixture
def lane(monkeypatch, tmp_path):
    monkeypatch.setenv("E2E_RESULTS_DIR", str(tmp_path))
    (tmp_path / "fd" / "general").mkdir(parents=True)
    return fdkit.FakeDecoderLane(RecordingCtl(), "general")


def test_an_unknown_lane_is_refused():
    # Act and Assert
    with pytest.raises(ValueError):
        fdkit.FakeDecoderLane(RecordingCtl(), "decoder-general")


def test_start_writes_the_script_removes_the_last_record_and_swaps_the_lanes_decoder(lane):
    # Arrange
    lane.record_path.write_text('{"kind":"hello"}\n', encoding="utf-8")

    # Act
    lane.start({"render_still": {"default": "silent"}})

    # Assert
    assert json.loads((lane.dir / "script.json").read_text(encoding="utf-8")) == {"render_still": {"default": "silent"}}
    assert not lane.record_path.exists()
    assert lane.ctl.calls == [("process-kill", {"service": "decoder-general", "op": "fake"}),
                              ("process-kill", {"service": "worker", "op": "kill"}),
                              ("process-kill", {"service": "worker", "op": "start"})]


def test_stop_without_a_worker_restart_only_swaps_the_decoder_back(lane):
    # Act
    lane.stop(restart_worker=False)

    # Assert
    assert lane.ctl.calls == [("process-kill", {"service": "decoder-general", "op": "real"})]


def test_a_script_that_is_not_an_object_is_refused_before_any_call(lane):
    # Act
    with pytest.raises(ValueError):
        lane.start(["silent"])

    # Assert
    assert lane.ctl.calls == []


def test_record_leaves_out_a_last_line_fd_is_still_writing(lane):
    # Arrange
    lane.record_path.write_text('{"kind":"hello","conn":1}\n{"kind":"self_ch', encoding="utf-8")

    # Act and Assert
    assert lane.record() == [{"kind": "hello", "conn": 1}]


@pytest.mark.parametrize("seen, wanted, expected", [
    (["hello", "self_check", "probe"], ["hello", "self_check"], True),
    (["hello", "cancel", "self_check"], ["hello", "self_check"], True),
    (["self_check", "hello"], ["hello", "self_check"], False),
    ([], ["hello"], False),
])
def test_contains_in_order_matches_kinds_in_their_order(seen, wanted, expected):
    # Act and Assert
    assert fdkit.contains_in_order(seen, wanted) is expected


def test_wait_for_raises_with_the_record_when_the_kinds_do_not_show(lane, monkeypatch):
    # Arrange
    lane.record_path.write_text('{"kind":"hello","conn":1}\n', encoding="utf-8")
    monkeypatch.setattr(fdkit, "POLL_SECONDS", 0.01)

    # Act and Assert
    with pytest.raises(AssertionError, match="self_check.*Permission denied"):
        lane.wait_for(["hello", "self_check"], 0.05)
    assert lane.ctl.calls[-1] == ("inspect", {"logs": "fd-general"})


def test_job_requests_keeps_only_the_job_kinds_of_a_record(lane):
    # Arrange
    lane.record_path.write_text('{"kind":"hello"}\n{"kind":"self_check"}\n{"kind":"probe"}\n{"kind":"cancel"}\n'
                                '{"kind":"render_still"}\n', encoding="utf-8")

    # Act and Assert
    assert [line["kind"] for line in fdkit.job_requests(lane)] == ["probe", "render_still"]

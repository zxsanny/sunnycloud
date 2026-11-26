"""Unit tests of the stackkit helpers of the worker suite (harness H2): each one checks its arguments before it calls
e2e-ctl, and builds the call the ctl action expects. No stack needed: the ctl transport is replaced."""
import pytest

import stackkit


@pytest.fixture
def ctl(monkeypatch, tmp_path):
    token = tmp_path / "ctl_token"
    token.write_text("unit-test-ctl-token\n", encoding="utf-8")
    monkeypatch.setenv("E2E_CTL_URL", "http://ctl.invalid:8088")
    monkeypatch.setenv("E2E_CTL_TOKEN_FILE", str(token))
    client = stackkit.Ctl()
    client.posts = []
    client.answers = []

    def fake_post(action, params):
        client.posts.append((action, params))
        return client.answers.pop(0) if client.answers else (200, {"result": {"processes": []}})

    monkeypatch.setattr(client, "_post", fake_post)
    return client


@pytest.mark.parametrize("call", [
    lambda c: c.worker_ops("/internal/v1/test/clock-offset"),
    lambda c: c.decoder_processes("worker"),
    lambda c: c.kill_decoder("api"),
    lambda c: c.start_decoder("decoder"),
    lambda c: c.rotate_worker_token("swap"),
    lambda c: c.timing_settings("db"),
])
def test_a_helper_refuses_an_argument_outside_its_list_before_calling_ctl(ctl, call):
    # Act
    with pytest.raises(ValueError):
        call(ctl)

    # Assert
    assert ctl.posts == []


def test_worker_ops_asks_the_workers_ops_port(ctl):
    # Arrange
    ctl.answers = [(200, {"result": {"connection": "open", "status": 503, "headers": {}, "body": "not ready"}})]

    # Act
    answer = ctl.worker_ops("/readyz")

    # Assert
    assert answer == (503, "not ready")
    assert ctl.posts == [("internal-http", {"service": "worker", "port": 9090, "path": "/readyz"})]


@pytest.mark.parametrize("reply", [(409, {"error": {"code": "service_not_running"}}), (200, {"result": {"connection": "refused"}})])
def test_worker_ops_reports_a_worker_without_a_listener_as_status_zero(ctl, reply):
    # Arrange
    ctl.answers = [reply]

    # Act and Assert
    assert ctl.worker_ops("/healthz") == (0, "")


def test_worker_metrics_parses_the_samples_with_their_labels(ctl):
    # Arrange
    text = ('# HELP sunnycloud_build_info build info\n# TYPE sunnycloud_build_info gauge\nsunnycloud_build_info 1\n'
            'sunnycloud_worker_selfcheck_ok{slot="general"} 0\n')
    ctl.answers = [(200, {"result": {"connection": "open", "status": 200, "headers": {}, "body": text}})]

    # Act
    samples = ctl.worker_metrics()

    # Assert
    assert samples == {"sunnycloud_build_info": 1.0, 'sunnycloud_worker_selfcheck_ok{slot="general"}': 0.0}


@pytest.mark.parametrize("method, action, params", [
    ("decoder_processes", "inspect", {"processes": "decoder-general"}),
    ("kill_decoder", "process-kill", {"service": "decoder-general", "op": "kill"}),
    ("start_decoder", "process-kill", {"service": "decoder-general", "op": "start"}),
])
def test_a_decoder_helper_calls_its_ctl_action(ctl, method, action, params):
    # Act
    getattr(ctl, method)("decoder-general")

    # Assert
    assert ctl.posts == [(action, params)]


@pytest.mark.parametrize("stage", ["next", "drop-old"])
def test_rotate_worker_token_restarts_the_api_with_the_stage(ctl, stage):
    # Act
    ctl.rotate_worker_token(stage)

    # Assert
    assert ctl.posts == [("process-kill", {"service": "api", "op": "restart", "worker_token": stage})]


def test_timing_settings_asks_inspect_for_the_service(ctl):
    # Arrange
    ctl.answers = [(200, {"result": {"settings": {"SUNNYCLOUD_JOB_LEASE": "6s"}}})]

    # Act
    settings = ctl.timing_settings("worker")

    # Assert
    assert settings == {"SUNNYCLOUD_JOB_LEASE": "6s"}
    assert ctl.posts == [("inspect", {"settings": "worker"})]


def test_internal_names_the_api_unless_told_otherwise(ctl):
    # Arrange
    ctl.answers = [(200, {"result": {"connection": "refused"}})]

    # Act
    ctl.internal(9091, "/test/clock", method="POST")

    # Assert
    assert ctl.posts[0][1]["service"] == "api"


def test_parse_metrics_refuses_a_line_that_is_not_a_sample():
    # Act and Assert
    with pytest.raises(ValueError):
        stackkit.parse_metrics("garbage\n")

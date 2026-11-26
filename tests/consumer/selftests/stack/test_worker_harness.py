"""The worker-suite harness on the running stack (AZ-4383): the timings of the harness profile reach the api and the
worker, and the e2e-ctl actions of the worker suite work and refuse what they do not allow.

Run in every profile: under `core` the timings are the core ones (the H2 override must not leak into `core`), under
`h2` they are those of 09 tests.md harness H2."""
import os
import time

WORKER_READY_SECONDS = 90
POLL_SECONDS = 0.5
CORE = {
    "api": {"SUNNYCLOUD_JOB_LEASE": "10s", "SUNNYCLOUD_WORKER_STATUS_INTERVAL": None},
    "worker": {"SUNNYCLOUD_JOB_LEASE": "10s", "SUNNYCLOUD_WORKER_CLAIM_POLL_INTERVAL": None,
               "SUNNYCLOUD_WORKER_STATUS_INTERVAL": None, "SUNNYCLOUD_DECODER_SILENCE_TIMEOUT": None,
               "SUNNYCLOUD_DECODER_CANCEL_GRACE": None, "SUNNYCLOUD_PHOTO_JOB_TIMEOUT": "10s",
               "SUNNYCLOUD_WORKER_SHUTDOWN_GRACE": None},
}
H2 = {
    "api": {"SUNNYCLOUD_JOB_LEASE": "6s", "SUNNYCLOUD_WORKER_STATUS_INTERVAL": "1s"},
    "worker": {"SUNNYCLOUD_JOB_LEASE": "6s", "SUNNYCLOUD_WORKER_CLAIM_POLL_INTERVAL": "100ms",
               "SUNNYCLOUD_WORKER_STATUS_INTERVAL": "1s", "SUNNYCLOUD_DECODER_SILENCE_TIMEOUT": "1s",
               "SUNNYCLOUD_DECODER_CANCEL_GRACE": "500ms", "SUNNYCLOUD_PHOTO_JOB_TIMEOUT": "5s",
               "SUNNYCLOUD_WORKER_SHUTDOWN_GRACE": "3s"},
}


def wait_healthy(ctl, service, seconds=WORKER_READY_SECONDS):
    deadline = time.monotonic() + seconds
    while True:
        health = ctl.call("inspect", service=service)["health"]
        if health == "healthy" or time.monotonic() > deadline:
            return health
        time.sleep(POLL_SECONDS)


def refused(ctl, action, params):
    """The status of a call that must be refused, read without failing the test on it."""
    status, body = ctl.try_call(action, **params)
    return status, body.get("error", {}).get("code")


def test_the_harness_profile_gives_the_api_and_the_worker_their_timings(stack_ctl):
    # Arrange
    expected = H2 if os.environ.get("E2E_PROFILE") == "h2" else CORE

    # Act
    settings = {service: stack_ctl.timing_settings(service) for service in expected}
    health = {service: wait_healthy(stack_ctl, service) for service in expected}

    # Assert
    for service, values in expected.items():
        assert {k: settings[service][k] for k in values} == values, service
    assert health == {"api": "healthy", "worker": "healthy"}


def test_the_workers_ops_port_answers_readiness_and_metrics_and_no_other_port(stack_ctl):
    # Act
    ready = stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS)
    metrics = stack_ctl.worker_metrics()
    other_port = refused(stack_ctl, "internal-http", {"service": "worker", "port": 8081, "path": "/"})
    bearer = refused(stack_ctl, "internal-http", {"service": "worker", "port": 9090, "path": "/readyz", "auth": "worker"})

    # Assert
    assert ready == 200
    assert [metrics[f'sunnycloud_worker_selfcheck_ok{{slot="{slot}"}}'] for slot in ("interactive", "general")] == [1.0, 1.0]
    assert (other_port, bearer) == ((400, "bad_request"), (400, "bad_request"))


def test_an_idle_decoder_runs_its_runner_and_no_media_tool(stack_ctl):
    # Act
    processes = stack_ctl.decoder_processes("decoder-interactive")
    other = refused(stack_ctl, "inspect", {"processes": "worker"})

    # Assert
    names = [p["comm"] for p in processes]
    assert "decoder-runner" in names, processes
    assert not {"ffmpeg", "ffprobe", "vips", "vipsthumbnail", "vipsheader"} & set(names), processes
    assert other == (400, "bad_request")


def test_a_killed_decoder_runner_ends_its_container_and_its_restart_leaves_the_worker_ready(stack_ctl):
    """The worker's reaction while the runner is gone (reconnect, `hello`, `self_check`) is IT-09-15's, not this test's."""
    # Arrange
    assert stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS) == 200
    started = stack_ctl.call("inspect", service="decoder-interactive")["started_at"]

    # Act
    stack_ctl.kill_decoder("decoder-interactive")
    try:
        after_kill = stack_ctl.call("inspect", service="decoder-interactive")["status"]
    finally:
        stack_ctl.start_decoder("decoder-interactive")  # the suites after this one need the decoder back
        healthy = wait_healthy(stack_ctl, "decoder-interactive")
        # Restart the supervisor, so the suites after this one do not wait for its reconnect backoff (up to 30 s).
        stack_ctl.restart_worker()
    ready_again = stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS)
    restarted = stack_ctl.call("inspect", service="decoder-interactive")
    bad_op = refused(stack_ctl, "process-kill", {"service": "decoder-interactive", "op": "restart"})

    # Assert
    assert after_kill == "exited"
    assert (healthy, ready_again) == ("healthy", 200)
    assert restarted["status"] == "running" and restarted["started_at"] != started
    assert bad_op == (400, "bad_request")


def test_a_worker_token_rotation_keeps_the_worker_until_the_old_token_is_dropped(stack_ctl):
    # Arrange
    assert stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS) == 200

    # Act
    try:
        stack_ctl.rotate_worker_token("next")
        ready_with_both = stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS)
        answer = stack_ctl.rotate_worker_token("drop-old")
        refused_old = stack_ctl.wait_worker_readyz(503, WORKER_READY_SECONDS)
    finally:
        # The worker reads the token file again at its start: with either outcome above it is then consistent with the api.
        stack_ctl.restart_worker()
    ready_with_new = stack_ctl.wait_worker_readyz(200, WORKER_READY_SECONDS)
    bad_stage = refused(stack_ctl, "process-kill", {"service": "api", "op": "restart", "worker_token": "swap"})

    # Assert
    assert ready_with_both == 200
    assert refused_old == 503
    assert ready_with_new == 200
    assert answer == {"service": "api", "op": "restart", "worker_token": "drop-old"}
    assert bad_stage == (400, "bad_request")

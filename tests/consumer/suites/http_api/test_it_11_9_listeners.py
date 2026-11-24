"""IT-11-9 (components/11_http_api/tests.md), Mode R on the test image: which route lives on which listener (AC-79)."""
import json

from stackkit import edge, problem_without_request_id

JOB = "00000000-0000-4000-8000-000000000009"
JSON = {"Content-Type": "application/json"}
WORKER_ID = {"X-Worker-Id": "it-11-9"}
# The three worker routes of the registry (`registry/worker/mod.rs`): the claim, the input read and the status write.
# The spec text writes the status route as `.../{id}/status`; the real one has no job id.
WORKER_ROUTES = [
    ("POST", "/api/v1/worker-jobs/claims", json.dumps({"lane": "image"})),
    ("GET", f"/api/v1/worker-jobs/{JOB}/input", None),
    ("PUT", "/api/v1/worker-jobs/status", json.dumps({"decoder_ok": True, "image_age_days": 0})),
]
UNKNOWN = "/api/v1/no-such-route-it-11-9"
NOT_WORKER_CREDENTIALS = {
    "none": {},
    "cookie": {"Cookie": "sc_session=not-a-worker"},
    "session bearer": {"Authorization": "Bearer session-token-not-a-worker"},
    "device bearer": {"Authorization": "Bearer device-token-not-a-worker"},
}


def send(ctl, port, route, auth="none", headers=None, worker_id=True):
    method, path, body = route
    sent = {**JSON, **(WORKER_ID if worker_id else {}), **(headers or {})}
    return ctl.internal(port, path, method=method, auth=auth, headers=sent, body=body)


def answer_shape(answer):
    return answer["status"], problem_without_request_id(answer["body"])


def test_worker_routes_are_404_on_the_public_listener_with_the_body_of_an_unknown_path(stack_ctl):
    # Arrange
    unknown = answer_shape(stack_ctl.internal(8080, UNKNOWN))

    # Act
    seen = {f"{m} {p}": answer_shape(send(stack_ctl, 8080, (m, p, b))) for m, p, b in WORKER_ROUTES}

    # Assert
    assert unknown[0] == 404
    assert seen == {f"{m} {p}": unknown for m, p, _ in WORKER_ROUTES}


def test_worker_routes_are_404_through_caddy_with_the_body_of_an_unknown_path():
    # Arrange
    unknown_status, unknown_body = edge(UNKNOWN)

    # Act
    seen = {f"{m} {p}": edge(p, method=m, data=b.encode() if b else None, headers={**JSON, **WORKER_ID}) for m, p, b in WORKER_ROUTES}

    # Assert
    assert unknown_status == 404
    expected = problem_without_request_id(unknown_body)
    assert {k: (s, problem_without_request_id(b)) for k, (s, b) in seen.items()} == {
        f"{m} {p}": (404, expected) for m, p, _ in WORKER_ROUTES
    }


def test_worker_listener_answers_403_alike_to_every_credential_that_is_not_the_worker_bearer(stack_ctl):
    # Arrange
    cases = [(name, route) for name in NOT_WORKER_CREDENTIALS for route in WORKER_ROUTES]

    # Act
    seen = {
        (name, route[0], route[1]): answer_shape(send(stack_ctl, 8081, route, headers=NOT_WORKER_CREDENTIALS[name]))
        for name, route in cases
    }

    # Assert
    assert {shape[0] for shape in seen.values()} == {403}
    assert len({json.dumps(shape, sort_keys=True) for shape in seen.values()}) == 1, "the 403 answers differ"


def test_worker_bearer_reaches_the_handlers_on_the_worker_listener(stack_ctl):
    # Arrange
    # A route that exists asks for the worker id before anything else; an unknown path would answer 404 instead.
    # With the id: the claim finds no job (204), the input read of a job this worker does not hold is a 409, the status write is accepted (204).
    expected = {"POST /api/v1/worker-jobs/claims": 204, f"GET /api/v1/worker-jobs/{JOB}/input": 409,
                "PUT /api/v1/worker-jobs/status": 204}

    # Act
    without_id = {f"{m} {p}": send(stack_ctl, 8081, (m, p, b), auth="worker", worker_id=False)["status"] for m, p, b in WORKER_ROUTES}
    with_id = {f"{m} {p}": send(stack_ctl, 8081, (m, p, b), auth="worker")["status"] for m, p, b in WORKER_ROUTES}

    # Assert
    assert without_id == {k: 400 for k in expected}, without_id
    assert with_id == expected, with_id


def test_ops_paths_answer_on_the_ops_listener_only(stack_ctl):
    # Arrange
    paths = ["/healthz", "/readyz", "/metrics"]

    # Act
    ops = {p: stack_ctl.internal(9090, p)["status"] for p in paths}
    public = {p: stack_ctl.internal(8080, p)["status"] for p in paths}
    through_caddy = {p: edge(p)[0] for p in paths}

    # Assert
    assert ops == {p: 200 for p in paths}
    assert public == {p: 404 for p in paths}
    assert through_caddy == {p: 404 for p in paths}


def test_test_image_serves_the_clock_endpoints_on_9091_and_8081(stack_ctl):
    # Arrange
    # Advancing by 0 s reads the offset and moves nothing for the tests that follow.
    advance = json.dumps({"advance_seconds": 0})

    # Act
    posted = stack_ctl.internal(9091, "/test/clock", method="POST", headers={"Content-Type": "application/json"}, body=advance)
    read = stack_ctl.internal(8081, "/internal/v1/test/clock-offset", auth="worker")

    # Assert
    assert posted["status"] == 200 and "offset_seconds" in json.loads(posted["body"]), posted
    assert read["status"] == 200 and json.loads(read["body"]) == json.loads(posted["body"]), read


def test_clock_endpoints_are_not_on_the_public_listener_or_through_caddy(stack_ctl):
    # Arrange
    paths = [("POST", "/test/clock"), ("GET", "/internal/v1/test/clock-offset")]

    # Act
    public = {p: stack_ctl.internal(8080, p, method=m)["status"] for m, p in paths}
    through_caddy = {p: edge(p, method=m, data=b"{}" if m == "POST" else None)[0] for m, p in paths}

    # Assert
    assert public == {p: 404 for _, p in paths}
    assert through_caddy == {p: 404 for _, p in paths}

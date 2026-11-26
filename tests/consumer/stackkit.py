"""What a black-box suite needs of the stack besides plain HTTP: the e2e-ctl client and the public edge (Caddy).

The consumer sits on client-net and reaches Caddy only. Listeners of the api that Caddy does not route (`:8081`,
`:9090`, `:9091`) are reached through the ctl action `internal-http`, one request per call; the worker bearer never
leaves ctl.
"""
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import pytest

READY_POLL_SECONDS = 1.0
DECODER_SERVICES = ("decoder-interactive", "decoder-general")
WORKER_OPS_PORT = 9090
WORKER_OPS_PATHS = ("/healthz", "/readyz", "/metrics")
WORKER_POLL_SECONDS = 0.5
# The api restarts of a worker token rotation (e2e-ctl `process-kill`): `next` adds a second accepted token,
# `drop-old` leaves only the new one, so a worker that still holds the old token gets 403 until it restarts.
WORKER_TOKEN_STAGES = ("next", "drop-old")


def required_env(name):
    value = os.environ.get(name)
    if not value:
        pytest.fail(f"harness misconfigured: {name} is not set (tests/run.sh sets it)", pytrace=False)
    return value


class Ctl:
    def __init__(self):
        self.url = required_env("E2E_CTL_URL").rstrip("/")
        token_file = Path(os.environ.get("E2E_CTL_TOKEN_FILE", "/run/secrets/ctl_token"))
        self.token = token_file.read_text(encoding="utf-8").strip()

    def _post(self, action, params):
        request = urllib.request.Request(f"{self.url}/v1/actions/{action}", data=json.dumps(params).encode(), method="POST")
        request.add_header("Authorization", f"Bearer {self.token}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=240) as answer:
                return answer.status, json.load(answer)
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read())

    def call(self, action, **params):
        status, body = self._post(action, params)
        if status != 200:
            pytest.fail(f"e2e-ctl {action} {params}: {status} {json.dumps(body)[:400]}", pytrace=False)
        return body["result"]

    def internal(self, port, path, method="GET", auth="none", headers=None, body=None, service="api"):
        """One request to `service` (the api, or the worker's ops port) on `port`; the answer is
        `{"connection": "refused"}` or status, headers and body."""
        params = {"service": service, "port": port, "path": path, "method": method, "auth": auth}
        if headers:
            params["headers"] = headers
        if body is not None:
            params["body"] = body
        return self.call("internal-http", **params)

    def ready(self):
        """`(status, body)` of `/readyz`; `(0, {})` while the api container has no address (stopped, restarting)."""
        status, body = self._post("internal-http", {"port": 9090, "path": "/readyz"})
        if status == 409:
            return 0, {}
        assert status == 200, f"internal-http: {status} {body}"
        answer = body["result"]
        if answer["connection"] != "open":
            return 0, {}
        return answer["status"], json.loads(answer["body"])

    def worker_ops(self, path):
        """`(status, body text)` of one GET on the worker's ops port; `(0, "")` while the worker container has no
        address or refuses the connection (stopped, restarting)."""
        if path not in WORKER_OPS_PATHS:
            raise ValueError(f"path is one of {WORKER_OPS_PATHS}, not {path!r}")
        status, body = self._post("internal-http", {"service": "worker", "port": WORKER_OPS_PORT, "path": path})
        if status == 409:
            return 0, ""
        assert status == 200, f"internal-http: {status} {body}"
        answer = body["result"]
        if answer["connection"] != "open":
            return 0, ""
        return answer["status"], answer["body"]

    def wait_worker_readyz(self, expected, seconds):
        """Polls the worker's `/readyz` until it answers `expected` (0 while it has no listener); returns the last
        status, so the caller asserts on it."""
        deadline = time.monotonic() + seconds
        while True:
            status, _ = self.worker_ops("/readyz")
            if status == expected or time.monotonic() > deadline:
                return status
            time.sleep(WORKER_POLL_SECONDS)

    def worker_metrics(self):
        """The samples of the worker's `/metrics`, keyed by name with labels
        (`sunnycloud_worker_selfcheck_ok{slot="interactive"}`)."""
        status, text = self.worker_ops("/metrics")
        assert status == 200, f"worker /metrics: {status}"
        return parse_metrics(text)

    def timing_settings(self, service):
        """The timing settings (`SUNNYCLOUD_JOB_LEASE`, ...) the api's or the worker's container was started with;
        `None` for one left unset."""
        if service not in ("api", "worker"):
            raise ValueError(f"service is api or worker, not {service!r}")
        return self.call("inspect", settings=service)["settings"]

    def decoder_processes(self, service):
        """The processes in a decoder container (`docker top` on the host): `[{"pid", "comm", "args"}]`."""
        _check_decoder(service)
        return self.call("inspect", processes=service)["processes"]

    def kill_decoder(self, service):
        """SIGKILL of a decoder: its runner ends as in a crash; `start_decoder` is the restart."""
        _check_decoder(service)
        return self.call("process-kill", service=service, op="kill")

    def start_decoder(self, service):
        _check_decoder(service)
        return self.call("process-kill", service=service, op="start")

    def rotate_worker_token(self, stage):
        """Restarts the api with the worker token rotated (`WORKER_TOKEN_STAGES`) and waits until it is healthy. No
        token reaches the consumer."""
        if stage not in WORKER_TOKEN_STAGES:
            raise ValueError(f"stage is one of {WORKER_TOKEN_STAGES}, not {stage!r}")
        return self.call("process-kill", service="api", op="restart", worker_token=stage)

    def wait_ready(self, seconds):
        deadline = time.monotonic() + seconds
        while True:
            status, body = self.ready()
            if status == 200 or time.monotonic() >= deadline:
                return status, body
            time.sleep(READY_POLL_SECONDS)


def _check_decoder(service):
    if service not in DECODER_SERVICES:
        raise ValueError(f"service is one of {DECODER_SERVICES}, not {service!r}")


def parse_metrics(text):
    """Prometheus text format to `{sample name with labels: value}`; comment lines are skipped."""
    samples = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        name, _, value = line.rpartition(" ")
        if not name:
            raise ValueError(f"not a metrics sample line: {line!r}")
        samples[name] = float(value)
    return samples


def edge(path, method="GET", data=None, headers=None):
    """One request through Caddy (the public URL); `(status, body text)` also for 4xx and 5xx."""
    request = urllib.request.Request(required_env("E2E_BASE_URL") + path, data=data, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(request, timeout=15) as answer:
            return answer.status, answer.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def problem_without_request_id(text):
    """A problem+json body with the per-request correlation id removed, for comparing two answers."""
    body = json.loads(text)
    body.pop("request_id", None)
    return body


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Browser:
    """A web client of the stack: cookies kept, redirects not followed (the test reads each `Location`)."""

    def __init__(self):
        import http.cookiejar

        self.jar = http.cookiejar.CookieJar()
        self._opener = urllib.request.build_opener(_NoRedirect, urllib.request.HTTPCookieProcessor(self.jar))

    def request(self, url, method="GET", body=None, headers=None, raw=False):
        """`body` is bytes (sent as is, octet-stream) or a JSON-able value. `raw` returns the answer's bytes, not text."""
        data = body if isinstance(body, bytes) else json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method, headers=headers or {})
        if data is not None and "Content-Type" not in request.headers:
            request.add_header("Content-Type", "application/octet-stream" if isinstance(body, bytes) else "application/json")
        try:
            with self._opener.open(request, timeout=15) as answer:
                data = answer.read()
                return answer.status, dict(answer.headers), data if raw else data.decode("utf-8", "replace")
        except urllib.error.HTTPError as error:
            data = error.read()
            return error.code, dict(error.headers), data if raw else data.decode("utf-8", "replace")

    def api(self, path, method="GET", body=None, headers=None):
        status, _, text = self.request(required_env("E2E_BASE_URL") + path, method, body, headers)
        return status, (json.loads(text) if text.strip().startswith(("{", "[")) else text)

    def fetch(self, path, headers=None):
        """`GET` of a file route: `(status, headers, bytes)`."""
        return self.request(required_env("E2E_BASE_URL") + path, "GET", None, headers, raw=True)

    def signed_in(self):
        """The cookies the browser holds authenticate the next request (the one that names the account)."""
        status, _ = self.api("/api/v1/accounts/me")
        return status == 200

    def begin(self, provider="oidc", return_to="/", client_kind="web", setup_token=None, invite_token=None,
              code_challenge=None, headers=None):
        """`POST login-attempts`: `(status, body)`; 201 carries the provider's `authorization_url`."""
        body = {"provider": provider, "return_to": return_to, "client_kind": client_kind}
        for name, value in (("setup_token", setup_token), ("invite_token", invite_token), ("code_challenge", code_challenge)):
            if value:
                body[name] = value
        return self.api("/api/v1/auth/login-attempts", "POST", body, headers)

    @staticmethod
    def consent(authorization_url, subject):
        """The user's step at the mock identity provider: the callback URL it redirects to."""
        hint = ("&" if "?" in authorization_url else "?") + "login_hint=" + subject
        status, headers, _ = _plain_get(authorization_url + hint)
        assert status == 302, f"the identity provider answered {status}"
        return headers["Location"]

    def callback(self, location):
        """`GET` the callback URL without following the redirect: `(status, Location or "")`."""
        status, headers, _ = self.request(location)
        return status, headers.get("Location", "")

    def sign_in(self, subject, setup_token=None, invite_token=None, client_kind="web", return_to="/", code_challenge=None):
        """The whole sign-in as `subject` of the mock identity provider; returns the final `Location`."""
        status, started = self.begin("oidc", return_to, client_kind, setup_token, invite_token, code_challenge)
        assert status == 201, f"login-attempts answered {status}: {started}"
        status, location = self.callback(self.consent(started["authorization_url"], subject))
        assert status == 302, f"the callback answered {status}"
        return location


def _plain_get(url):
    opener = urllib.request.build_opener(_NoRedirect)
    try:
        with opener.open(url, timeout=15) as answer:
            return answer.status, dict(answer.headers), answer.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, dict(error.headers), ""


def operator_token(ctl, *args):
    """Runs `sunnycloud <args>` in the api container and returns the `token:` value of its stdout."""
    result = ctl.call("exec-operator", args=list(args))
    assert result["exit_code"] == 0, f"{args}: exit {result['exit_code']}: {result['stderr']}"
    values = [line[len("token: "):] for line in result["stdout"].splitlines() if line.startswith("token: ")]
    assert len(values) == 1, f"{args}: stdout has {len(values)} token lines"
    return values[0]


def pkce_pair():
    """A PKCE verifier and its S256 challenge (RFC 7636)."""
    import base64
    import hashlib
    import secrets

    verifier = secrets.token_urlsafe(48)
    digest = hashlib.sha256(verifier.encode()).digest()
    return verifier, base64.urlsafe_b64encode(digest).rstrip(b"=").decode()


class FailureBudget:
    """The sign-in limit (AC-20): 10 failed steps per client address per minute, fixed. All the consumer's traffic is
    one address, so a test that makes failed steps asks first; this waits for the limiter's own window when the
    budget is used up. A restore of the golden snapshot restarts the api and so resets the limiter."""

    PER_WINDOW = 9
    WINDOW_SECONDS = 61

    def __init__(self):
        self._spent = []

    def reset(self):
        self._spent.clear()

    def spend(self, count=1):
        while True:
            now = time.monotonic()
            self._spent = [t for t in self._spent if now - t < self.WINDOW_SECONDS]
            if len(self._spent) + count <= self.PER_WINDOW:
                self._spent.extend([now] * count)
                return
            time.sleep(max(0.5, self.WINDOW_SECONDS - (now - self._spent[0])))


budget = FailureBudget()
READY_SECONDS = 90


def restore_and_wait(ctl):
    """The stack back to the golden snapshot (no accounts), ready for requests."""
    ctl.call("db-restore", snapshot="golden")
    status, body = ctl.wait_ready(READY_SECONDS)
    assert status == 200, f"the stack is not ready after the restore: {body}"
    budget.reset()


WORKER_OK_SECONDS = 75


def wait_worker_ok(ctl, seconds=WORKER_OK_SECONDS):
    """Waits for the media worker's status report to say its decoders passed the gate. The worker reports every
    `worker_status_interval` (30 s) and its first report after a start comes before the gate, so `doctor` fails
    `media_worker` for up to one interval after every worker start (a reset restarts it)."""
    deadline = time.monotonic() + seconds
    while True:
        status = ctl.call("db-probe", query="worker_status")
        if status["workers"] >= 1 and status["decoder_ok"] == status["workers"]:
            return status
        if time.monotonic() > deadline:
            raise AssertionError(f"the worker did not report healthy decoders within {seconds} s: {status}")
        time.sleep(1)


def advance_clock(ctl, seconds):
    """Moves the server's test clock (the `:9091` listener of the test image) and returns the new offset."""
    answer = ctl.internal(9091, "/test/clock", method="POST", headers={"Content-Type": "application/json"},
                          body=json.dumps({"advance_seconds": seconds}))
    assert answer["status"] == 200, answer
    return json.loads(answer["body"])["offset_seconds"]


def idp_fault(fault):
    """Scripts the mock identity provider's next token exchange (`None` clears it)."""
    request = urllib.request.Request(required_env("E2E_IDP_URL") + "/__control/fault",
                                     data=json.dumps({"fault": fault}).encode(), method="POST")
    with urllib.request.urlopen(request, timeout=10) as answer:
        assert answer.status == 200


PUBLIC_ORIGIN = "https://sunnycloud.test"


class Drive:
    """The files of a signed-in browser: folders, small uploads, edits, renames, moves, deletes. A cookie-authenticated
    write proves its origin (CSRF), so every call that changes something sends the public origin."""

    def __init__(self, browser):
        self.browser = browser

    def call(self, path, method="GET", body=None, headers=None):
        sent = dict(headers or {})
        if method != "GET":
            sent.setdefault("Origin", PUBLIC_ORIGIN)
        return self.browser.api(path, method, body, sent)

    def ok(self, expected, path, method="GET", body=None, headers=None):
        status, answer = self.call(path, method, body, headers)
        assert status in (expected if isinstance(expected, tuple) else (expected,)), f"{method} {path}: {status} {answer}"
        return answer

    def root(self):
        return self.ok(200, "/api/v1/nodes/root")["node_id"]

    def folder(self, parent_id, name):
        return self.ok(201, "/api/v1/nodes", "POST", {"parent_id": parent_id, "name": name})["node_id"]

    def upload(self, parent_id, name, data, base_revision=None):
        """A whole small file: staged, then committed. `base_revision` makes it an edit of that file."""
        import uuid

        from blake3 import blake3

        assert len(data) <= 2 * 1024 * 1024, "a small upload is one block of at most 2 MiB"
        staged = str(uuid.uuid4())
        # The hash of the one block (BLAKE3), in hex: the server checks the bytes against it.
        self.ok(200, f"/api/v1/staged-uploads/{staged}", "PUT", data, {"X-Blake3": blake3(data).hexdigest()})
        entry = {"staged_key": staged, "parent_id": parent_id, "name": name}
        if base_revision:
            entry["base_revision"] = base_revision
        answer = self.ok(200, "/api/v1/commits:batch", "POST", {"commit_id": str(uuid.uuid4()), "entries": [entry]})
        result = answer["entries"][0]
        assert result["status"] == "committed", result
        return {"node_id": result["node_id"], "revision_id": result["revision_id"]}

    def rename(self, node_id, name):
        return self.ok(200, f"/api/v1/nodes/{node_id}", "PATCH", {"name": name})

    def move(self, node_id, parent_id):
        return self.ok(200, f"/api/v1/nodes/{node_id}", "PATCH", {"parent_id": parent_id})

    def delete(self, node_id):
        self.ok(204, f"/api/v1/nodes/{node_id}", "DELETE")


def device_token(browser):
    """A desktop client's bearer: the grant of RFC 8628, approved by the signed-in `browser`. Returns the access token."""
    anonymous = Browser()
    status, grant = anonymous.api("/api/v1/device-grants", "POST", {"device_name": "e2e-desktop", "platform": "linux"})
    assert status == 200, grant
    drive = Drive(browser)
    info = drive.ok(200, f"/api/v1/device-grants?user_code={urllib.parse.quote(grant['user_code'])}")
    drive.ok(204, f"/api/v1/device-grants/{info['grant_id']}", "PATCH", {"decision": "approve"})
    status, token = anonymous.api("/api/v1/device-tokens", "POST", {"device_code": grant["device_code"]})
    assert status == 200, token
    return token["access_token"]


VOLATILE_HEADERS = {"date", "x-request-id", "request-id", "server-timing", "set-cookie"}


def refusal_shape(status, headers, text):
    """What a refused request looks like to a caller, without what changes per request (id, date)."""
    body = json.loads(text) if text.strip().startswith("{") else text
    if isinstance(body, dict):
        body.pop("request_id", None)
    kept = {k.lower(): v for k, v in headers.items() if k.lower() not in VOLATILE_HEADERS and k.lower() != "content-length"}
    return status, kept, body

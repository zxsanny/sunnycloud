"""What a black-box suite needs of the stack besides plain HTTP: the e2e-ctl client and the public edge (Caddy).

The consumer sits on client-net and reaches Caddy only. Listeners of the api that Caddy does not route (`:8081`,
`:9090`, `:9091`) are reached through the ctl action `internal-http`, one request per call; the worker bearer never
leaves ctl.
"""
import json
import os
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

READY_POLL_SECONDS = 1.0


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

    def internal(self, port, path, method="GET", auth="none", headers=None, body=None):
        """One request to the api on `port`; the answer is `{"connection": "refused"}` or status, headers and body."""
        params = {"port": port, "path": path, "method": method, "auth": auth}
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

    def wait_ready(self, seconds):
        deadline = time.monotonic() + seconds
        while True:
            status, body = self.ready()
            if status == 200 or time.monotonic() >= deadline:
                return status, body
            time.sleep(READY_POLL_SECONDS)


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

    def request(self, url, method="GET", body=None, headers=None):
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method, headers=headers or {})
        if data is not None:
            request.add_header("Content-Type", "application/json")
        try:
            with self._opener.open(request, timeout=15) as answer:
                return answer.status, dict(answer.headers), answer.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as error:
            return error.code, dict(error.headers), error.read().decode("utf-8", "replace")

    def api(self, path, method="GET", body=None, headers=None):
        status, _, text = self.request(required_env("E2E_BASE_URL") + path, method, body, headers)
        return status, (json.loads(text) if text.strip().startswith(("{", "[")) else text)

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

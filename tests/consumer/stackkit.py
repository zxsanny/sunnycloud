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

    def request(self, url, method="GET", body=None):
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        if data is not None:
            request.add_header("Content-Type", "application/json")
        try:
            with self._opener.open(request, timeout=15) as answer:
                return answer.status, dict(answer.headers), answer.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as error:
            return error.code, dict(error.headers), error.read().decode("utf-8", "replace")

    def api(self, path, method="GET", body=None):
        status, headers, text = self.request(required_env("E2E_BASE_URL") + path, method, body)
        return status, (json.loads(text) if text.strip().startswith(("{", "[")) else text)

    def signed_in(self):
        """The cookies the browser holds authenticate the next request (the one that names the account)."""
        status, _ = self.api("/api/v1/accounts/me")
        return status == 200

    def sign_in(self, subject, setup_token=None, invite_token=None):
        """The whole web sign-in as `subject` of the mock identity provider; returns the final `Location`."""
        body = {"provider": "oidc", "return_to": "/", "client_kind": "web"}
        if setup_token:
            body["setup_token"] = setup_token
        if invite_token:
            body["invite_token"] = invite_token
        status, started = self.api("/api/v1/auth/login-attempts", "POST", body)
        assert status == 201, f"login-attempts answered {status}: {started}"
        url = started["authorization_url"]
        hint = ("&" if "?" in url else "?") + "login_hint=" + subject
        idp_status, idp_headers, _ = _plain_get(url + hint)
        assert idp_status == 302, f"the identity provider answered {idp_status}"
        status, headers, _ = self.request(idp_headers["Location"])
        assert status == 302, f"the callback answered {status}"
        return headers["Location"]


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

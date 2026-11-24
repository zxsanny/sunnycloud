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

    def call(self, action, **params):
        request = urllib.request.Request(f"{self.url}/v1/actions/{action}", data=json.dumps(params).encode(), method="POST")
        request.add_header("Authorization", f"Bearer {self.token}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=240) as answer:
                return json.load(answer)["result"]
        except urllib.error.HTTPError as error:
            pytest.fail(f"e2e-ctl {action} {params}: {error.code} {error.read().decode()[:400]}", pytrace=False)

    def internal(self, port, path, method="GET", auth="none", headers=None, body=None):
        """One request to the api on `port`; the answer is `{"connection": "refused"}` or status, headers and body."""
        params = {"port": port, "path": path, "method": method, "auth": auth}
        if headers:
            params["headers"] = headers
        if body is not None:
            params["body"] = body
        return self.call("internal-http", **params)

    def ready(self):
        answer = self.internal(9090, "/readyz")
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

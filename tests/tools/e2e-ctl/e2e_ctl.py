#!/usr/bin/env python3
"""e2e-ctl skeleton: the host-side control service of the black-box harness.

Every action of the Fault injectors table is declared in actions.json and answers the typed error
`not_implemented`; real actions arrive with E-31. A bearer token is required, and every request,
refused or not, writes exactly one JSON line to the audit log before the response is sent.

    POST /v1/actions/<name>   Authorization: Bearer <token>   ->  501 {"error": {"code": "not_implemented", ...}}
"""
import argparse
import hmac
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MAX_BODY_BYTES = 1024 * 1024
WILDCARD_ADDRESSES = {"", "0.0.0.0", "::"}
ACTION_PREFIX = "/v1/actions/"


def load_actions(path):
    return frozenset(json.loads(Path(path).read_text(encoding="utf-8"))["actions"])


def load_token(path):
    token = Path(path).read_text(encoding="utf-8").strip()
    if not token:
        raise ValueError(f"token file {path} is empty")
    return token


class AuditLog:
    def __init__(self, path):
        self._path = Path(path)
        self._lock = threading.Lock()

    def write(self, **fields):
        line = json.dumps({"ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), **fields}, sort_keys=True)
        with self._lock, self._path.open("a", encoding="utf-8") as out:
            out.write(line + "\n")


class CtlServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, token, actions, audit):
        super().__init__(address, CtlHandler)
        self.token = token
        self.actions = actions
        self.audit = audit


class CtlHandler(BaseHTTPRequestHandler):
    server_version = "e2e-ctl-skeleton"

    def _authenticate(self):
        header = self.headers.get("Authorization")
        if header is None:
            return "missing"
        scheme, _, presented = header.partition(" ")
        if scheme.lower() != "bearer" or not hmac.compare_digest(presented.strip().encode(), self.server.token.encode()):
            return "invalid"
        return "ok"

    def _route(self, auth):
        if auth != "ok":
            return 401, "unauthorized", "a valid bearer token is required", None
        if not self.path.startswith(ACTION_PREFIX):
            return 404, "not_found", f"no such resource: {self.path}", None
        action = self.path[len(ACTION_PREFIX):]
        if action not in self.server.actions:
            return 404, "unknown_action", f"no such action: {action}", action
        if self.command != "POST":
            return 405, "method_not_allowed", "actions are called with POST", action
        return 501, "not_implemented", f"action '{action}' has no implementation yet", action

    def _handle(self):
        length = int(self.headers.get("Content-Length") or 0)
        too_large = length > MAX_BODY_BYTES
        if not too_large and length:
            self.rfile.read(length)
        auth = self._authenticate()
        status, code, message, action = (413, "payload_too_large", "request body too large", None) if too_large else self._route(auth)
        if action is None and self.path.startswith(ACTION_PREFIX):
            action = self.path[len(ACTION_PREFIX):]
        self.server.audit.write(client=self.client_address[0], method=self.command, path=self.path,
                                action=action, auth=auth, status=status, error=code, body_bytes=length)
        body = json.dumps({"error": {"code": code, "message": message, "action": action}}).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = do_HEAD = _handle


def build_server(bind, port, token, actions, audit_path):
    if bind in WILDCARD_ADDRESSES:
        raise ValueError("bind address must be one concrete address (the ctl-net gateway), not a wildcard")
    return CtlServer((bind, port), token, actions, AuditLog(audit_path))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bind", required=True, help="the ctl-net gateway address; wildcards are refused")
    parser.add_argument("--port", type=int, default=8088)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--audit-log", required=True)
    parser.add_argument("--actions-file", default=str(Path(__file__).with_name("actions.json")))
    args = parser.parse_args(argv)
    server = build_server(args.bind, args.port, load_token(args.token_file), load_actions(args.actions_file), args.audit_log)
    print(f"e2e-ctl skeleton listening on {args.bind}:{args.port}, audit log {args.audit_log}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("e2e-ctl skeleton stopped", file=sys.stderr, flush=True)
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

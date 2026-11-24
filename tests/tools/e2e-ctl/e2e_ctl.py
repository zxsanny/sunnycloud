#!/usr/bin/env python3
"""e2e-ctl: the host-side control service of the black-box harness.

Every action of the Fault injectors table is declared in actions.json. The ones in ctl_actions.py run and answer
200 with their result; the others answer the typed error `not_implemented`. A bearer token is required, and every
request, refused or not, writes exactly one JSON line to the audit log before the response is sent. The audit line
names the action and its status, never the body (a body may carry a credential).

    POST /v1/actions/<name>   Authorization: Bearer <token>   JSON body = the action's parameters
        ->  200 {"result": {...}}   or   {"error": {"code": ..., "message": ..., "action": ...}} with the status
"""
import argparse
import hmac
import json
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import ctl_actions

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

    def __init__(self, address, token, actions, audit, handlers=None):
        super().__init__(address, CtlHandler)
        self.token = token
        self.actions = actions
        self.audit = audit
        self.handlers = handlers or {}


class CtlHandler(BaseHTTPRequestHandler):
    server_version = "e2e-ctl"

    def _authenticate(self):
        header = self.headers.get("Authorization")
        if header is None:
            return "missing"
        scheme, _, presented = header.partition(" ")
        if scheme.lower() != "bearer" or not hmac.compare_digest(presented.strip().encode(), self.server.token.encode()):
            return "invalid"
        return "ok"

    def _route(self, auth, raw_body):
        if auth != "ok":
            return 401, "unauthorized", "a valid bearer token is required", None, None
        if not self.path.startswith(ACTION_PREFIX):
            return 404, "not_found", f"no such resource: {self.path}", None, None
        action = self.path[len(ACTION_PREFIX):]
        if action not in self.server.actions:
            return 404, "unknown_action", f"no such action: {action}", action, None
        if self.command != "POST":
            return 405, "method_not_allowed", "actions are called with POST", action, None
        handler = self.server.handlers.get(action)
        if handler is None:
            return 501, "not_implemented", f"action '{action}' has no implementation yet", action, None
        try:
            return 200, None, None, action, handler(ctl_actions.parse_body(raw_body))
        except ctl_actions.ActionError as error:
            return error.status, error.code, str(error), action, None
        except (OSError, subprocess.SubprocessError) as error:
            # A hung command or a lost connection is still an answer, and still audited.
            return 500, "action_failed", f"{type(error).__name__}: {error}"[:400], action, None

    def _handle(self):
        length = int(self.headers.get("Content-Length") or 0)
        too_large = length > MAX_BODY_BYTES
        raw_body = self.rfile.read(length) if length and not too_large else b""
        auth = self._authenticate()
        status, code, message, action, result = (413, "payload_too_large", "request body too large", None, None) if too_large else self._route(auth, raw_body)
        if action is None and self.path.startswith(ACTION_PREFIX):
            action = self.path[len(ACTION_PREFIX):]
        self.server.audit.write(client=self.client_address[0], method=self.command, path=self.path,
                                action=action, auth=auth, status=status, error=code, body_bytes=length)
        payload = {"result": result} if code is None else {"error": {"code": code, "message": message, "action": action}}
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = do_HEAD = _handle


def build_server(bind, port, token, actions, audit_path, handlers=None):
    if bind in WILDCARD_ADDRESSES:
        raise ValueError("bind address must be one concrete address (the ctl-net gateway), not a wildcard")
    return CtlServer((bind, port), token, actions, AuditLog(audit_path), handlers)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bind", required=True, help="the ctl-net gateway address; wildcards are refused")
    parser.add_argument("--port", type=int, default=8088)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--audit-log", required=True)
    parser.add_argument("--actions-file", default=str(Path(__file__).with_name("actions.json")))
    parser.add_argument("--project", required=True, help="the compose project of the stack")
    parser.add_argument("--compose-file", required=True)
    parser.add_argument("--secrets-dir", required=True, help="the generated secrets (the worker token never leaves ctl)")
    args = parser.parse_args(argv)
    handlers = ctl_actions.Actions(args.project, args.compose_file, args.secrets_dir).handlers()
    server = build_server(args.bind, args.port, load_token(args.token_file), load_actions(args.actions_file),
                          args.audit_log, handlers)
    print(f"e2e-ctl listening on {args.bind}:{args.port}, audit log {args.audit_log}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("e2e-ctl stopped", file=sys.stderr, flush=True)
    finally:
        server.server_close()


if __name__ == "__main__":
    main()

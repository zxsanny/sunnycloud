"""The actions of e2e-ctl that have an implementation (environment.md, Fault injectors). Each takes the JSON body of
the call and returns a JSON-able dict, or raises `ActionError` for a refused or failed call. The others keep
answering `not_implemented` in `e2e_ctl.py`.

    internal-http        one HTTP request from the host to a backend listener of a service of the stack
    db-outage            stop or start the database container
    db-restore           restore the database to the golden snapshot (`tests/run.sh reset`: api and worker restart)
    process-kill         SIGKILL the api or the worker, or start it again (a crash and its restart)
    db-probe             one of a fixed set of read-only count queries on the database (no free-form SQL)
    exec-operator        run `sunnycloud admin ...`, `doctor` or `key-check` inside the api container
    interrupted-upgrade  revert the latest migrations, or apply the pending ones, with the product's own `migrate`
"""
import http.client
import ipaddress
import json
import subprocess
import time
from pathlib import Path

HTTP_SERVICES = frozenset({"api"})
HTTP_PORTS = frozenset({8080, 8081, 9090, 9091})
HTTP_METHODS = frozenset({"GET", "POST", "PUT", "DELETE", "HEAD"})
OPERATOR_COMMANDS = frozenset({"admin", "doctor", "key-check"})
GOLDEN = "golden"
# The fixed queries of `db-probe`: each returns one row of integers, named by the keys, or (kind `list`) one text of
# comma-separated values.
LIST = "list"
PROBES = {
    # The rows an account has after sign-in: the account, its identity and the rows the lifecycle hooks create.
    "account_rows": (("accounts", "identities", "quota", "account_feeds"),
                     "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM identities), "
                     "(SELECT count(*) FROM quota), (SELECT count(*) FROM account_feeds)"),
    # The security events the sign-in and session rules write.
    "audit_events": (("sign_in", "abuse_trip", "refresh_reuse", "session_revoked", "share_created", "share_revoked",
                      "share_accessed", "account_deleted"),
                     "SELECT count(*) FILTER (WHERE event = 'sign_in'), count(*) FILTER (WHERE event = 'abuse_trip'), "
                     "count(*) FILTER (WHERE event = 'refresh_reuse'), count(*) FILTER (WHERE event = 'session_revoked'), "
                     "count(*) FILTER (WHERE event = 'share_created'), count(*) FILTER (WHERE event = 'share_revoked'), "
                     "count(*) FILTER (WHERE event = 'share_accessed'), count(*) FILTER (WHERE event = 'account_deleted') "
                     "FROM audit_log"),
    # Rows per table that an account's data lives in: after an account is erased none of them keeps a row of it.
    "table_rows": (("accounts", "identities", "sessions", "device_tokens", "nodes", "revisions", "blobs", "blob_gc",
                    "upload_sessions", "share_links", "jobs", "derived", "quota", "account_feeds", "login_codes"),
                   "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM identities), (SELECT count(*) FROM sessions), "
                   "(SELECT count(*) FROM device_tokens), (SELECT count(*) FROM nodes), (SELECT count(*) FROM revisions), "
                   "(SELECT count(*) FROM blobs), (SELECT count(*) FROM blob_gc), (SELECT count(*) FROM upload_sessions), "
                   "(SELECT count(*) FROM share_links), (SELECT count(*) FROM jobs), (SELECT count(*) FROM derived), "
                   "(SELECT count(*) FROM quota), (SELECT count(*) FROM account_feeds), (SELECT count(*) FROM login_codes)"),
    "account_states": (("active", "deleting"),
                       "SELECT count(*) FILTER (WHERE state = 'active'), count(*) FILTER (WHERE state = 'deleting') FROM accounts"),
    # What a share link row stores: the column names show a token hash and no token.
    "share_link_columns": (LIST,
                           "SELECT coalesce(string_agg(column_name, ',' ORDER BY column_name), '') "
                           "FROM information_schema.columns WHERE table_name = 'share_links'"),
    # The distinct keys of the detail of those events: the audit log holds ids, codes and integers, so a key that
    # names a secret or a person would show here.
    "audit_detail_keys": (LIST,
                          "SELECT coalesce(string_agg(k, ',' ORDER BY k), '') FROM (SELECT DISTINCT jsonb_object_keys(detail) AS k "
                          "FROM audit_log WHERE detail IS NOT NULL AND jsonb_typeof(detail) = 'object') s"),
}
KILLABLE = frozenset({"api", "worker"})
HEADER_NAMES = frozenset({"authorization", "cookie", "content-type", "x-sunnycloud-client", "x-forwarded-for", "x-worker-id"})
BODY_LIMIT = 4096
REQUEST_TIMEOUT_SECONDS = 10
COMMAND_TIMEOUT_SECONDS = 180
DB_HEALTHY_TIMEOUT_SECONDS = 120
POLL_SECONDS = 1.0
WORKER_TOKEN_SECRET = "worker_api_token"


class ActionError(Exception):
    def __init__(self, status, code, message):
        super().__init__(message)
        self.status = status
        self.code = code


def run_command(args, timeout=COMMAND_TIMEOUT_SECONDS):
    done = subprocess.run(args, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout, check=False)
    return done.returncode, done.stdout, done.stderr


def send_request(address, method, path, headers, body):
    """One request; `None` when the connection is refused or reset (what a closed listener looks like)."""
    connection = http.client.HTTPConnection(address[0], address[1], timeout=REQUEST_TIMEOUT_SECONDS)
    try:
        connection.request(method, path, body=body, headers=headers)
        answer = connection.getresponse()
        return answer.status, {k.lower(): v for k, v in answer.getheaders()}, answer.read(BODY_LIMIT)
    except ConnectionError:
        return None
    finally:
        connection.close()


class Actions:
    def __init__(self, project, compose_file, secrets_dir, reset_script="", run=run_command, request=send_request,
                 sleep=time.sleep):
        self.project = project
        self.compose_file = str(compose_file)
        self.reset_script = str(reset_script)
        self.secrets_dir = Path(secrets_dir)
        self._run = run
        self._request = request
        self._sleep = sleep

    def handlers(self):
        return {
            "internal-http": self.internal_http,
            "db-outage": self.db_outage,
            "db-probe": self.db_probe,
            "process-kill": self.process_kill,
            "db-restore": self.db_restore,
            "exec-operator": self.exec_operator,
            "interrupted-upgrade": self.interrupted_upgrade,
        }

    def _compose(self, *args):
        return ["docker", "compose", "-p", self.project, "-f", self.compose_file, "--profile", "core", *args]

    def _checked(self, args):
        code, out, err = self._run(args)
        if code != 0:
            raise ActionError(500, "command_failed", f"{' '.join(args[:6])}... exited {code}: {err.strip()[-400:]}")
        return out

    def _container(self, service):
        container = self._checked(self._compose("ps", "-q", service)).strip()
        if not container:
            raise ActionError(409, "service_not_running", f"service {service} has no container")
        return container

    def _address(self, service):
        template = f'{{{{(index .NetworkSettings.Networks "{self.project}_backend").IPAddress}}}}'
        ip = self._checked(["docker", "inspect", "-f", template, self._container(service)]).strip()
        try:
            return str(ipaddress.ip_address(ip))
        except ValueError as error:
            # docker prints `invalid IP` (or nothing) for a container that is stopped or restarting.
            raise ActionError(409, "service_not_on_backend", f"service {service} has no address on the backend network") from error

    def internal_http(self, params):
        service, port = params.get("service", "api"), params.get("port")
        method, path = params.get("method", "GET"), params.get("path", "")
        if service not in HTTP_SERVICES or port not in HTTP_PORTS or method not in HTTP_METHODS or not str(path).startswith("/"):
            raise ActionError(400, "bad_request", f"service {HTTP_SERVICES}, port {sorted(HTTP_PORTS)}, a method and a path are required")
        headers = {}
        for name, value in (params.get("headers") or {}).items():
            if name.lower() not in HEADER_NAMES or not isinstance(value, str):
                raise ActionError(400, "bad_request", f"header {name!r} is not allowed")
            headers[name] = value
        if params.get("auth") == "worker":
            token = (self.secrets_dir / WORKER_TOKEN_SECRET).read_text(encoding="utf-8").strip()
            headers["Authorization"] = f"Bearer {token}"
        elif params.get("auth") not in (None, "none"):
            raise ActionError(400, "bad_request", "auth is `none` or `worker`")
        body = params["body"].encode() if isinstance(params.get("body"), str) else None
        answer = self._request((self._address(service), port), method, path, headers, body)
        if answer is None:
            return {"connection": "refused"}
        status, answer_headers, answer_body = answer
        return {"connection": "open", "status": status, "headers": answer_headers, "body": answer_body.decode("utf-8", "replace")}

    def db_outage(self, params):
        op = params.get("op")
        if op == "stop":
            self._checked(self._compose("stop", "db"))
            return {"db": "stopped"}
        if op != "start":
            raise ActionError(400, "bad_request", "op is `stop` or `start`")
        self._checked(self._compose("start", "db"))
        container = self._container("db")
        deadline = time.monotonic() + DB_HEALTHY_TIMEOUT_SECONDS
        template = "{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}"
        while time.monotonic() < deadline:
            if self._checked(["docker", "inspect", "-f", template, container]).strip() == "healthy":
                return {"db": "healthy"}
            self._sleep(POLL_SECONDS)
        raise ActionError(504, "db_not_healthy", f"the database was not healthy within {DB_HEALTHY_TIMEOUT_SECONDS} s")

    def process_kill(self, params):
        service, op = params.get("service"), params.get("op")
        if service not in KILLABLE or op not in ("kill", "start"):
            raise ActionError(400, "bad_request", f"service is one of {sorted(KILLABLE)}, op is `kill` or `start`")
        if op == "kill":
            self._checked(self._compose("kill", "-s", "KILL", service))
        else:
            self._checked(self._compose("start", service))
        return {"service": service, "op": op}

    def db_probe(self, params):
        probe = PROBES.get(params.get("query"))
        if probe is None:
            raise ActionError(400, "bad_request", f"query is one of {sorted(PROBES)}")
        keys, sql = probe
        out = self._checked(self._compose("exec", "-T", "db", "psql", "-U", "postgres", "-d", "sunnycloud", "-At", "-F", ",", "-c", sql))
        if keys == LIST:
            return {"values": [v for v in out.strip().split(",") if v]}
        values = out.strip().split(",")
        if len(values) != len(keys) or not all(v.lstrip("-").isdigit() for v in values):
            raise ActionError(500, "probe_failed", f"unexpected probe output: {out.strip()[:200]}")
        return dict(zip(keys, (int(v) for v in values)))

    def db_restore(self, params):
        if params.get("snapshot") != GOLDEN:
            raise ActionError(400, "bad_request", "snapshot is `golden`, the one snapshot the stack takes")
        self._checked(["bash", self.reset_script, "reset"])
        return {"restored": GOLDEN}

    def exec_operator(self, params):
        args = params.get("args")
        if not isinstance(args, list) or not args or not all(isinstance(a, str) and a and "\0" not in a for a in args) \
                or args[0] not in OPERATOR_COMMANDS:
            raise ActionError(400, "bad_request", f"args is a list of texts starting with one of {sorted(OPERATOR_COMMANDS)}")
        code, out, err = self._run(self._compose("exec", "-T", "api", "sunnycloud", *args))
        return {"exit_code": code, "stdout": out[-BODY_LIMIT:], "stderr": err[-BODY_LIMIT:]}

    def interrupted_upgrade(self, params):
        op = params.get("op")
        if op == "revert":
            steps = params.get("steps", 1)
            if not isinstance(steps, int) or isinstance(steps, bool) or steps < 1:
                raise ActionError(400, "bad_request", "steps is a positive integer")
            command = ["migrate", "revert", "--steps", str(steps), "--yes"]
        elif op == "apply":
            command = ["migrate", "run"]
        else:
            raise ActionError(400, "bad_request", "op is `revert` or `apply`")
        output = self._checked(self._compose("run", "--rm", "--no-deps", "-T", "migrate", *command))
        return {"op": op, "output": output[-BODY_LIMIT:]}


def parse_body(raw):
    if not raw:
        return {}
    try:
        value = json.loads(raw)
    except ValueError as error:
        raise ActionError(400, "bad_request", f"the body is not JSON: {error}") from error
    if not isinstance(value, dict):
        raise ActionError(400, "bad_request", "the body is a JSON object")
    return value

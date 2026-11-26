"""The actions of e2e-ctl that have an implementation (environment.md, Fault injectors). Each takes the JSON body of
the call and returns a JSON-able dict, or raises `ActionError` for a refused or failed call. The others keep
answering `not_implemented` in `e2e_ctl.py`.

    internal-http        one HTTP request from the host to a backend listener of a service of the stack
    db-outage            stop or start the database container
    db-restore           restore the database to the golden snapshot (`tests/run.sh reset`: api and worker restart), or
                         take a named `pg_dump` of it and load that dump back (a backup and its restore, the api stopped)
    process-kill         SIGKILL the api or the worker, or start it again (a crash and its restart)
    db-probe             one of a fixed set of read-only count queries on the database (no free-form SQL)
    exec-operator        run `sunnycloud admin ...`, `doctor`, `key-check` or `scrub` inside the api container (or in a one-off
                         api container while `serve` is down), or `migrate ...` in a one-off `migrate` container;
                         optionally watching the api's listening sockets
    inspect              a container's state or its recent log, a decoder scratch volume, a secret scan (given tokens and the master key in
                         every SUT log, the audit rows and host `ps`) or the api's database role (DDL, migrator secret)
    interrupted-upgrade  revert the latest migrations, or apply the pending ones, with the product's own `migrate`
"""
import base64
import http.client
import ipaddress
import json
import subprocess
import threading
import time
from pathlib import Path

HTTP_SERVICES = frozenset({"api"})
HTTP_PORTS = frozenset({8080, 8081, 9090, 9091})
HTTP_METHODS = frozenset({"GET", "POST", "PUT", "DELETE", "HEAD"})
OPERATOR_COMMANDS = frozenset({"admin", "doctor", "key-check", "scrub"})
MIGRATE_COMMANDS = frozenset({"migrate"})
# A listening socket in /proc/net/tcp{,6}: state 0A.
LISTEN_STATE = "0A"
LISTEN_SAMPLE_SECONDS = 0.05
MASTER_KEY_SECRET = "master_key"
API_URL_SECRET = "db_api_url"
MIGRATOR_URL_SECRET = "db_migrator_url"
TOKEN_MIN_LENGTH = 16
# The services whose logs `secret-scan` reads: the SUT and its dependencies, not the consumer (its pytest output is the
# test's own, not the product's).
LOG_SERVICES = ("api", "worker", "decoder-interactive", "decoder-general", "caddy", "db", "mock-idp", "migrate", "keycheck")
# The DDL the api role must not run; inside a transaction that is rolled back, so an allowed statement changes nothing.
ROLE_STATEMENTS = {
    "drop_table": "BEGIN; DROP TABLE audit_log; ROLLBACK;",
    "create_table": "BEGIN; CREATE TABLE e2e_role_probe (x int); ROLLBACK;",
}
GOLDEN = "golden"
DUMP_NAME_CHARS = frozenset("abcdefghijklmnopqrstuvwxyz0123456789-")
# The dump's path inside the database container: one per call (a file another uid left there cannot be overwritten),
# removed as root after use.
DUMP_IN_DB = "/tmp/e2e-{name}-{serial}.dump"
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
    # The media pipeline (AZ-3661): job states and outcomes, derived rows by kind, blobs that nothing refers to (neither a
    # revision nor a derived row: a surplus derived blob), and the supervisor's own status rows.
    "media_jobs": (("queued", "running", "derived", "skipped_compliant", "obsolete", "dead"),
                   "SELECT count(*) FILTER (WHERE status = 'queued'), count(*) FILTER (WHERE status = 'running'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'derived'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'skipped_compliant'), "
                   "count(*) FILTER (WHERE status = 'succeeded' AND outcome = 'obsolete'), "
                   "count(*) FILTER (WHERE status = 'dead') FROM jobs"),
    "job_attempts": (("most",), "SELECT coalesce(max(attempts), 0) FROM jobs"),
    "derived_rows": (("thumb", "preview", "transcode", "total"),
                     "SELECT count(*) FILTER (WHERE kind = 'thumb'), count(*) FILTER (WHERE kind = 'preview'), "
                     "count(*) FILTER (WHERE kind = 'transcode'), count(*) FROM derived"),
    "blob_refs": (("blobs", "committed", "unreferenced"),
                  "SELECT count(*), count(*) FILTER (WHERE state = 'committed'), count(*) FILTER (WHERE state = 'committed' AND "
                  "NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = blobs.id) AND "
                  "NOT EXISTS (SELECT 1 FROM derived d WHERE d.blob_id = blobs.id)) FROM blobs"),
    "worker_status": (("workers", "decoder_ok"),
                      "SELECT count(*), count(*) FILTER (WHERE decoder_ok) FROM worker_status"),
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
    # What a destructive operator command changes besides rows: the applied migrations and the feed epochs (AZ-3815 AC-6).
    "schema_and_epochs": (("migrations_applied", "epoch_sum", "epoch_max"),
                          "SELECT (SELECT count(*) FROM _sqlx_migrations WHERE success), "
                          "(SELECT coalesce(sum(epoch), 0) FROM account_feeds), (SELECT coalesce(max(epoch), 0) FROM account_feeds)"),
}
KILLABLE = frozenset({"api", "worker"})
PAUSABLE = frozenset({"decoder-interactive", "decoder-general"})
INSPECTABLE = frozenset({"api", "worker", "decoder-interactive", "decoder-general"})
SCRATCH_LANES = frozenset({"interactive", "general"})
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


def run_command(args, timeout=COMMAND_TIMEOUT_SECONDS, input=None):
    """`input`, when given, is the command's stdin (a secret never goes on a command line, where `ps` shows it)."""
    stdin = None if input is not None else subprocess.DEVNULL
    done = subprocess.run(args, stdin=stdin, input=input, capture_output=True, text=True, timeout=timeout, check=False)
    return done.returncode, done.stdout, done.stderr


def listening_ports(proc_net):
    """The local ports in state LISTEN of the text of /proc/net/tcp and /proc/net/tcp6."""
    ports = set()
    for line in proc_net.splitlines():
        fields = line.split()
        if len(fields) > 3 and fields[3] == LISTEN_STATE and ":" in fields[1]:
            ports.add(int(fields[1].rsplit(":", 1)[1], 16))
    return ports


def watch_listeners(sampler_args, command_args, run=run_command):
    """Runs `command_args` while a sampler (a throwaway container in the api's network namespace) prints /proc/net/tcp
    and /proc/net/tcp6 in blocks ended by `---`. Returns the command's result and the ports listening before it
    started and at any sample while it ran."""
    sampler = subprocess.Popen(sampler_args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True)
    try:
        blocks, current = [], []
        for line in sampler.stdout:
            if line.strip() == "---":
                blocks.append("\n".join(current))
                break
            current.append(line)
        before = listening_ports(blocks[0]) if blocks else None
        if before is None:
            raise ActionError(500, "sampler_failed", f"the listener sampler gave no sample: {sampler.stderr.read()[-300:]}")
        done = {}

        def finish():
            done["result"] = run(command_args)

        worker = threading.Thread(target=finish)
        worker.start()
        during, current = set(), []
        while worker.is_alive():
            line = sampler.stdout.readline()
            if not line:
                break
            if line.strip() == "---":
                during |= listening_ports("\n".join(current))
                current = []
            else:
                current.append(line)
        worker.join()
        return done["result"], before, during
    finally:
        sampler.kill()
        sampler.wait()


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
        self._serial = 0

    def handlers(self):
        return {
            "internal-http": self.internal_http,
            "db-outage": self.db_outage,
            "db-probe": self.db_probe,
            "process-kill": self.process_kill,
            "inspect": self.inspect,
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
        """`kill` (SIGKILL) or `start` an api or worker; `pause` or `unpause` a decoder (the container is frozen, so a job
        that is being decoded stays in the decoder and the supervisor waits: a deterministic "during decoding")."""
        service, op = params.get("service"), params.get("op")
        if not ((service in KILLABLE and op in ("kill", "start")) or (service in PAUSABLE and op in ("pause", "unpause"))):
            raise ActionError(400, "bad_request", f"`kill` or `start` for {sorted(KILLABLE)}, `pause` or `unpause` for {sorted(PAUSABLE)}")
        if op == "kill":
            self._checked(self._compose("kill", "-s", "KILL", service))
        else:
            self._checked(self._compose(op, service))
        return {"service": service, "op": op}

    def inspect(self, params):
        """`{"service"}`: the container's state, start time and restart count (a hostile-class decoder exit shows as a new
        start time). `{"scratch": "interactive"|"general"}`: the top-level names in that decoder scratch volume, listed
        by a throwaway container with the volume read-only (the isolation check of AC-71: nothing left after a job)."""
        if params.get("secret_scan") is not None:
            return self.secret_scan(params["secret_scan"])
        if params.get("logs") is not None:
            if params["logs"] not in INSPECTABLE:
                raise ActionError(400, "bad_request", f"logs is one of {sorted(INSPECTABLE)}")
            text = self._checked(self._compose("logs", "--no-color", "--no-log-prefix", params["logs"]))
            return {"log_tail": text[-BODY_LIMIT:]}
        if params.get("db_role") is not None:
            if params["db_role"] != "api":
                raise ActionError(400, "bad_request", "db_role is `api`")
            return self.db_role_check()
        if params.get("scratch") is not None:
            lane = params["scratch"]
            if lane not in SCRATCH_LANES:
                raise ActionError(400, "bad_request", f"scratch is one of {sorted(SCRATCH_LANES)}")
            image = self._checked(["docker", "inspect", "-f", "{{.Config.Image}}", self._container("db")]).strip()
            listing = self._checked(["docker", "run", "--rm", "--network", "none", "-v", f"{self.project}_scratch_{lane}:/s:ro",
                                     "--entrypoint", "ls", image, "-A", "/s"])
            return {"entries": [name for name in listing.split("\n") if name]}
        service = params.get("service")
        if service not in INSPECTABLE:
            raise ActionError(400, "bad_request", f"service is one of {sorted(INSPECTABLE)}")
        template = "{{.State.Status}}|{{.State.StartedAt}}|{{.RestartCount}}|{{if .State.Health}}{{.State.Health.Status}}{{end}}"
        status, started_at, restarts, health = self._checked(["docker", "inspect", "-f", template, self._container(service)]).strip().split("|")
        return {"status": status, "started_at": started_at, "restart_count": int(restarts), "health": health}

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
        """`{"snapshot": "golden"}`: the golden snapshot (`tests/run.sh reset`). `{"op": "dump", "name"}`: `pg_dump -Fc`
        of the database, kept on the host next to the run's secrets (deleted at teardown). `{"op": "load", "name"}`:
        that dump restored over the database with `pg_restore --clean` as the superuser; the caller stops the api
        first, as an operator restoring a backup does."""
        op, name = params.get("op"), params.get("name", "")
        if op is None:
            if params.get("snapshot") != GOLDEN:
                raise ActionError(400, "bad_request", "snapshot is `golden`, the one snapshot the stack takes")
            self._checked(["bash", self.reset_script, "reset"])
            return {"restored": GOLDEN}
        if op not in ("dump", "load") or not name or not set(name) <= DUMP_NAME_CHARS:
            raise ActionError(400, "bad_request", "op is `dump` or `load`, name is lower-case letters, digits and -")
        dumps = self.secrets_dir / "dumps"
        host_file = dumps / f"{name}.dump"
        db = self._container("db")
        self._serial += 1
        in_db = DUMP_IN_DB.format(name=name, serial=self._serial)
        as_root = ("exec", "-T", "-u", "root", "db")
        if op == "dump":
            dumps.mkdir(exist_ok=True)
            self._checked(self._compose("exec", "-T", "db", "pg_dump", "-U", "postgres", "-Fc", "-f", in_db, "sunnycloud"))
            self._checked(["docker", "cp", f"{db}:{in_db}", str(host_file)])
            self._checked(self._compose(*as_root, "rm", "-f", in_db))
            return {"dumped": name, "bytes": host_file.stat().st_size}
        if not host_file.is_file():
            raise ActionError(404, "not_found", f"no dump named {name}")
        self._checked(["docker", "cp", str(host_file), f"{db}:{in_db}"])
        self._checked(self._compose(*as_root, "chmod", "a+r", in_db))
        self._checked(self._compose("exec", "-T", "db", "pg_restore", "-U", "postgres", "-d", "sunnycloud", "--clean",
                                    "--if-exists", "--exit-on-error", "--single-transaction", in_db))
        self._checked(self._compose(*as_root, "rm", "-f", in_db))
        return {"loaded": name}

    def exec_operator(self, params):
        """`service` `api` (default): `docker compose exec -T api sunnycloud <args>`, stdin not a TTY and empty. `migrate`:
        a one-off container of the `migrate` service (the only one holding the migrator secret). With `watch_listeners`
        (api only) the result adds the api's listening ports before the command and any new one seen while it ran."""
        args, service = params.get("args"), params.get("service", "api")
        allowed = {"api": OPERATOR_COMMANDS, "api-oneoff": OPERATOR_COMMANDS, "migrate": MIGRATE_COMMANDS}.get(service)
        if allowed is None or not isinstance(args, list) or not args \
                or not all(isinstance(a, str) and a and "\0" not in a for a in args) or args[0] not in allowed:
            raise ActionError(400, "bad_request", f"args is a list of texts starting with one of {sorted(OPERATOR_COMMANDS)} "
                                                  f"(service api) or {sorted(MIGRATE_COMMANDS)} (service migrate)")
        if service in ("migrate", "api-oneoff"):
            command = self._compose("run", "--rm", "--no-deps", "-T", "migrate" if service == "migrate" else "api", *args)
        else:
            command = self._compose("exec", "-T", "api", "sunnycloud", *args)
        if not params.get("watch_listeners"):
            code, out, err = self._run(command)
            return {"exit_code": code, "stdout": out[-BODY_LIMIT:], "stderr": err[-BODY_LIMIT:]}
        if service != "api":
            raise ActionError(400, "bad_request", "watch_listeners is for service api")
        (code, out, err), before, during = watch_listeners(self._sampler_args(), command, self._run)
        return {"exit_code": code, "stdout": out[-BODY_LIMIT:], "stderr": err[-BODY_LIMIT:],
                "listening_before": sorted(before), "new_listening": sorted(during - before)}

    def _sampler_args(self):
        image = self._checked(["docker", "inspect", "-f", "{{.Config.Image}}", self._container("db")]).strip()
        loop = f"while :; do cat /proc/net/tcp /proc/net/tcp6; echo ---; sleep {LISTEN_SAMPLE_SECONDS}; done"
        return ["docker", "run", "--rm", "-i", "--network", f"container:{self._container('api')}", "--entrypoint", "sh",
                image, "-c", loop]

    def _secret(self, name):
        return (self.secrets_dir / name).read_text(encoding="utf-8").strip()

    def secret_scan(self, params):
        """How often each given token (and the master key, as hex, base64 and raw text) occurs in the logs of every
        container of the stack, in the text of every audit row, in the host's `ps` output and in `outputs` (the stdout
        and stderr the consumer was shown, so the key, which the consumer never holds, is looked for there too). Only counts come back:
        a needle is never echoed, and the ctl audit line holds no body."""
        tokens = params.get("tokens") if isinstance(params, dict) else None
        if not isinstance(tokens, list) or not all(isinstance(t, str) and len(t) >= TOKEN_MIN_LENGTH for t in tokens):
            raise ActionError(400, "bad_request", f"tokens is a list of texts of at least {TOKEN_MIN_LENGTH} characters")
        key = (self.secrets_dir / MASTER_KEY_SECRET).read_bytes()
        needles = {f"token_{i}": t for i, t in enumerate(tokens)}
        needles.update({"master_key_hex": key.hex(), "master_key_base64": base64.b64encode(key).decode(),
                        "master_key_base64url": base64.urlsafe_b64encode(key).decode().rstrip("=")})
        outputs = params.get("outputs", "")
        if not isinstance(outputs, str):
            raise ActionError(400, "bad_request", "outputs is the text of what the consumer was shown")
        places = {
            "outputs": outputs,
            "logs": self._checked(self._compose("logs", "--no-color", "--no-log-prefix", *LOG_SERVICES)),
            "audit": self._checked(self._compose("exec", "-T", "db", "psql", "-U", "postgres", "-d", "sunnycloud", "-At",
                                                 "-c", "SELECT row_to_json(a)::text FROM audit_log a")),
            "ps": self._checked(["ps", "-eo", "args"]),
        }
        raw_key = key.decode("latin-1")
        counts = {place: {label: text.count(needle) for label, needle in needles.items()} for place, text in places.items()}
        for place, text in places.items():
            counts[place]["master_key_raw"] = text.count(raw_key)
        return {"counts": counts}

    def db_role_check(self):
        """Runs each statement of `ROLE_STATEMENTS` with the api's own connection string (handed to psql on stdin, never
        on a command line) and reports `denied` with the SQLSTATE, or `allowed`; then the names of the api container's
        environment variables and mounted secrets that mention the migrator."""
        url = self._secret(API_URL_SECRET)
        results = {}
        for label, statement in ROLE_STATEMENTS.items():
            script = 'read -r url; exec psql "$url" -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -At -c "$1"'
            code, _, err = self._run(self._compose("exec", "-T", "db", "sh", "-c", script, "sh", statement), input=url + "\n")
            if code == 0:
                results[label] = {"result": "allowed"}
                continue
            words = err.replace(":", " ").split()
            sqlstate = next((w for w in words if len(w) == 5 and w.isalnum() and w[:2].isdigit()), "")
            results[label] = {"result": "denied", "sqlstate": sqlstate}
        container = self._container("api")
        env_names = [entry.split("=", 1)[0] for entry in
                     json.loads(self._checked(["docker", "inspect", "-f", "{{json .Config.Env}}", container]) or "[]")]
        mounts = json.loads(self._checked(["docker", "inspect", "-f", "{{json .Mounts}}", container]) or "[]")
        secret_names = sorted(m["Destination"].rsplit("/", 1)[1] for m in mounts if m.get("Destination", "").startswith("/run/secrets/"))
        migrator = [n for n in env_names + secret_names if "MIGRATOR" in n.upper()]
        return {"statements": results, "api_secrets": secret_names, "migrator_names": migrator}

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

"""Unit tests of the implemented e2e-ctl actions with a fake docker and a fake HTTP client; no docker needed."""
import json
import subprocess
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

import ctl_actions
import e2e_ctl

TOKEN = "unit-test-token"
WORKER_TOKEN = "worker-secret-value"


class FakeDocker:
    def __init__(self, health=("healthy",)):
        self.calls = []
        self.health = list(health)

    def __call__(self, args):
        self.calls.append(args)
        if "ps" in args:
            return 0, "container-1\n", ""
        if "inspect" in args and "-f" in args and "Networks" in args[args.index("-f") + 1]:
            return 0, "172.30.0.5\n", ""
        if "inspect" in args:
            return 0, self.health.pop(0) + "\n", ""
        return 0, "ok\n", ""


class ActionsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        (Path(self.tmp.name) / "worker_api_token").write_text(WORKER_TOKEN + "\n", encoding="utf-8")
        self.docker = FakeDocker()
        self.requests = []
        self.answer = (404, {"content-type": "application/problem+json"}, b'{"code":"not_found"}')
        self.actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=self.docker,
                                           request=self.fake_request, sleep=lambda _: None)

    def tearDown(self):
        self.tmp.cleanup()

    def fake_request(self, address, method, path, headers, body):
        self.requests.append((address, method, path, headers, body))
        return self.answer

    def test_internal_http_asks_the_container_on_the_backend_network(self):
        # Arrange and Act
        result = self.actions.internal_http({"port": 8081, "method": "POST", "path": "/api/v1/worker-jobs/claims"})

        # Assert
        self.assertEqual(self.requests, [(("172.30.0.5", 8081), "POST", "/api/v1/worker-jobs/claims", {}, None)])
        self.assertEqual((result["connection"], result["status"]), ("open", 404))

    def test_internal_http_worker_auth_adds_the_bearer_and_never_returns_it(self):
        # Arrange and Act
        result = self.actions.internal_http({"port": 8081, "path": "/x", "auth": "worker"})

        # Assert
        self.assertEqual(self.requests[0][3], {"Authorization": f"Bearer {WORKER_TOKEN}"})
        self.assertNotIn(WORKER_TOKEN, json.dumps(result))

    def test_internal_http_reports_a_refused_connection_as_a_result(self):
        # Arrange
        self.answer = None

        # Act
        result = self.actions.internal_http({"port": 9091, "path": "/test/clock"})

        # Assert
        self.assertEqual(result, {"connection": "refused"})

    def test_internal_http_refuses_what_the_table_does_not_name(self):
        # Arrange
        bad = [{"port": 22, "path": "/"}, {"port": 8080, "path": "x"}, {"port": 8080, "path": "/", "method": "TRACE"},
               {"port": 8080, "path": "/", "headers": {"X-Evil": "1"}}, {"port": 8080, "path": "/", "auth": "root"},
               {"port": 8080, "path": "/", "service": "db"}]

        for params in bad:
            with self.subTest(params=params):
                # Act and Assert
                with self.assertRaises(ctl_actions.ActionError) as raised:
                    self.actions.internal_http(params)
                self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.requests, [])

    def test_db_outage_stop_and_start_waits_for_a_healthy_database(self):
        # Arrange
        self.docker.health = ["starting", "starting", "healthy"]

        # Act
        stopped = self.actions.db_outage({"op": "stop"})
        started = self.actions.db_outage({"op": "start"})

        # Assert
        self.assertEqual((stopped, started), ({"db": "stopped"}, {"db": "healthy"}))
        self.assertEqual(self.docker.health, [])

    def test_interrupted_upgrade_reverts_and_applies_with_the_products_migrate(self):
        # Arrange and Act
        self.actions.interrupted_upgrade({"op": "revert", "steps": 2})
        self.actions.interrupted_upgrade({"op": "apply"})

        # Assert
        tails = [call[call.index("migrate"):] for call in self.docker.calls if "run" in call]
        self.assertEqual(tails, [["migrate", "migrate", "revert", "--steps", "2", "--yes"], ["migrate", "migrate", "run"]])

    def test_exec_operator_runs_the_binary_in_the_api_container_and_returns_the_exit_code(self):
        # Arrange
        self.docker_result = (3, "out\n", "err\n")
        runner = lambda args: (self.docker.calls.append(args) or self.docker_result)
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        result = actions.exec_operator({"args": ["admin", "setup-token"]})

        # Assert
        self.assertEqual(result, {"exit_code": 3, "stdout": "out\n", "stderr": "err\n"})
        self.assertEqual(self.docker.calls[0][-5:], ["-T", "api", "sunnycloud", "admin", "setup-token"])

    def test_exec_operator_refuses_commands_outside_the_allow_list(self):
        # Arrange
        bad = [{}, {"args": []}, {"args": ["serve"]}, {"args": ["migrate", "revert"]}, {"args": ["admin", 1]}, {"args": "admin"}]

        for params in bad:
            with self.subTest(params=params):
                # Act and Assert
                with self.assertRaises(ctl_actions.ActionError) as raised:
                    self.actions.exec_operator(params)
                self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.docker.calls, [])

    def test_exec_operator_runs_migrate_commands_in_a_one_off_migrate_container_only(self):
        # Arrange
        runner = lambda args: (self.docker.calls.append(args) or (3, "", "confirmation required\n"))
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        result = actions.exec_operator({"service": "migrate", "args": ["migrate", "revert"]})
        with self.assertRaises(ctl_actions.ActionError) as raised:
            actions.exec_operator({"service": "migrate", "args": ["admin", "setup-token"]})

        # Assert
        self.assertEqual(result["exit_code"], 3)
        self.assertEqual(self.docker.calls[0][-7:], ["run", "--rm", "--no-deps", "-T", "migrate", "migrate", "revert"])
        self.assertEqual(raised.exception.status, 400)

    def test_db_restore_dumps_to_the_host_and_loads_only_a_known_dump(self):
        # Arrange
        def runner(args):
            self.docker.calls.append(args)
            if args[:2] == ["docker", "cp"] and ":" in args[2]:
                Path(args[3]).write_bytes(b"PGDMP")
            if "ps" in args:
                return 0, "container-1\n", ""
            return 0, "", ""
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        dumped = actions.db_restore({"op": "dump", "name": "ds22"})
        loaded = actions.db_restore({"op": "load", "name": "ds22"})
        with self.assertRaises(ctl_actions.ActionError) as missing:
            actions.db_restore({"op": "load", "name": "other"})
        with self.assertRaises(ctl_actions.ActionError) as bad:
            actions.db_restore({"op": "load", "name": "../x"})

        # Assert
        self.assertEqual((dumped, loaded), ({"dumped": "ds22", "bytes": 5}, {"loaded": "ds22"}))
        restore = next(call for call in self.docker.calls if "pg_restore" in call)
        self.assertIn("--clean", restore)
        self.assertEqual((missing.exception.status, bad.exception.status), (404, 400))

    def test_listening_ports_reads_the_listen_rows_of_proc_net_tcp(self):
        # Arrange
        text = ("  sl  local_address rem_address   st\n"
                "   0: 00000000:1F90 00000000:0000 0A 00000000:00000000\n"
                "   1: 0100007F:2382 00000000:0000 0A 00000000:00000000\n"
                "   2: 0100007F:2382 0100007F:D431 01 00000000:00000000\n"
                "   0: 00000000000000000000000000000000:1F91 00000000000000000000000000000000:0000 0A 0\n")

        # Act
        ports = ctl_actions.listening_ports(text)

        # Assert
        self.assertEqual(ports, {8080, 9090, 8081})

    def test_secret_scan_returns_counts_only_and_never_a_needle(self):
        # Arrange
        key = bytes(range(32))
        (Path(self.tmp.name) / "master_key").write_bytes(key)
        token = "tok_" + "a" * 20
        outputs = {"logs": f"line {token}\nkey {key.hex()}\n", "psql": f'{{"detail": "{token}"}}\n', "ps": "sunnycloud serve\n"}

        def runner(args):
            self.docker.calls.append(args)
            if "ps" in args and "-q" in args:
                return 0, "container-1\n", ""
            if "logs" in args:
                return 0, outputs["logs"], ""
            if "psql" in args:
                return 0, outputs["psql"], ""
            return 0, outputs["ps"], ""
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        result = actions.inspect({"secret_scan": {"tokens": [token]}})
        with self.assertRaises(ctl_actions.ActionError) as short:
            actions.inspect({"secret_scan": {"tokens": ["short"]}})

        # Assert
        counts = result["counts"]
        self.assertEqual((counts["logs"]["token_0"], counts["audit"]["token_0"], counts["ps"]["token_0"]), (1, 1, 0))
        self.assertEqual(counts["logs"]["master_key_hex"], 1)
        self.assertNotIn(token, json.dumps(result))
        self.assertNotIn(key.hex(), json.dumps(result))
        self.assertEqual(short.exception.status, 400)
        logs_call = next(call for call in self.docker.calls if "logs" in call)
        self.assertNotIn("consumer", logs_call)

    def test_db_role_check_feeds_the_url_on_stdin_and_reports_the_sqlstate(self):
        # Arrange
        url = "postgres://api:secret-pw@db:5432/sunnycloud"
        (Path(self.tmp.name) / "db_api_url").write_text(url + "\n", encoding="utf-8")
        seen = []

        def runner(args, input=None):
            seen.append((args, input))
            if "psql" in " ".join(args):
                return 1, "", "ERROR:  42501: must be owner of table audit_log\n"
            if "Env" in " ".join(args):
                return 0, '["SUNNYCLOUD_PUBLIC_URL=https://x", "PATH=/bin"]', ""
            if "Mounts" in " ".join(args):
                return 0, '[{"Destination": "/run/secrets/db_api_url"}, {"Destination": "/var/lib/sunnycloud/blobs"}]', ""
            return 0, "container-1\n", ""
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        result = actions.inspect({"db_role": "api"})

        # Assert
        self.assertEqual(result["statements"]["drop_table"], {"result": "denied", "sqlstate": "42501"})
        self.assertEqual(result["api_secrets"], ["db_api_url"])
        self.assertEqual(result["migrator_names"], [])
        psql_calls = [(args, given) for args, given in seen if "psql" in " ".join(args)]
        self.assertTrue(psql_calls and all(url not in " ".join(args) and given == url + "\n" for args, given in psql_calls))

    def test_db_probe_returns_the_counts_of_a_fixed_query_and_refuses_any_other(self):
        # Arrange
        runner = lambda args: (self.docker.calls.append(args) or (0, "2,2,2,2\n", ""))
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        counts = actions.db_probe({"query": "account_rows"})
        with self.assertRaises(ctl_actions.ActionError) as raised:
            actions.db_probe({"query": "DROP TABLE accounts"})

        # Assert
        self.assertEqual(counts, {"accounts": 2, "identities": 2, "quota": 2, "account_feeds": 2})
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(len(self.docker.calls), 1)

    def test_db_probe_list_query_returns_its_values(self):
        # Arrange
        runner = lambda args: (0, "client_kind,ip\n", "")
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        result = actions.db_probe({"query": "audit_detail_keys"})

        # Assert
        self.assertEqual(result, {"values": ["client_kind", "ip"]})

    def test_db_probe_media_queries_return_their_named_counts(self):
        # Arrange
        runner = lambda args: (0, "1,0,3,1,0,2\n", "")
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        jobs = actions.db_probe({"query": "media_jobs"})

        # Assert
        self.assertEqual(jobs, {"queued": 1, "running": 0, "derived": 3, "skipped_compliant": 1, "obsolete": 0, "dead": 2})

    def test_inspect_reports_a_container_and_lists_a_scratch_volume_and_refuses_the_rest(self):
        # Arrange
        def runner(args):
            self.docker.calls.append(args)
            if "ps" in args:
                return 0, "container-1\n", ""
            if "inspect" in args and "{{.Config.Image}}" in args:
                return 0, "postgres:18\n", ""
            if "inspect" in args:
                return 0, "running|2026-11-24T14:07:27Z|0|healthy\n", ""
            return 0, "job-a\njob-b\n", ""

        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=runner)

        # Act
        state = actions.inspect({"service": "decoder-interactive"})
        scratch = actions.inspect({"scratch": "general"})
        with self.assertRaises(ctl_actions.ActionError) as db:
            actions.inspect({"service": "db"})
        with self.assertRaises(ctl_actions.ActionError) as lane:
            actions.inspect({"scratch": "../etc"})

        # Assert
        self.assertEqual(state, {"status": "running", "started_at": "2026-11-24T14:07:27Z", "restart_count": 0, "health": "healthy"})
        self.assertEqual(scratch, {"entries": ["job-a", "job-b"]})
        self.assertEqual((db.exception.status, lane.exception.status), (400, 400))
        self.assertIn("proj_scratch_general:/s:ro", self.docker.calls[-1])

    def test_process_kill_kills_or_starts_only_the_api_and_the_worker(self):
        # Arrange
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=self.docker)

        # Act
        actions.process_kill({"service": "api", "op": "kill"})
        actions.process_kill({"service": "worker", "op": "start"})
        actions.process_kill({"service": "decoder-general", "op": "pause"})
        with self.assertRaises(ctl_actions.ActionError):
            actions.process_kill({"service": "db", "op": "kill"})
        with self.assertRaises(ctl_actions.ActionError):
            actions.process_kill({"service": "decoder-general", "op": "kill"})

        # Assert
        self.assertEqual([c[-4:] for c in self.docker.calls[:1]], [["kill", "-s", "KILL", "api"]])
        self.assertEqual(self.docker.calls[1][-2:], ["start", "worker"])
        self.assertEqual(self.docker.calls[2][-2:], ["pause", "decoder-general"])
        self.assertEqual(len(self.docker.calls), 3)

    def test_db_restore_runs_the_reset_script_only_for_the_golden_snapshot(self):
        # Arrange
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, "/x/run.sh", run=self.docker)

        # Act
        result = actions.db_restore({"snapshot": "golden"})
        with self.assertRaises(ctl_actions.ActionError):
            actions.db_restore({})

        # Assert
        self.assertEqual(result, {"restored": "golden"})
        self.assertEqual(self.docker.calls, [["bash", "/x/run.sh", "reset"]])

    def test_a_failing_command_is_a_typed_error(self):
        # Arrange
        failing = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=lambda args: (1, "", "boom"))

        # Act and Assert
        with self.assertRaises(ctl_actions.ActionError) as raised:
            failing.db_outage({"op": "stop"})
        self.assertEqual((raised.exception.status, raised.exception.code), (500, "command_failed"))


class ServedActionsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.audit_path = Path(self.tmp.name) / "audit.log"
        actions = e2e_ctl.load_actions(Path(__file__).with_name("actions.json"))
        handlers = {"db-outage": lambda params: {"echo": params}}
        self.server = e2e_ctl.build_server("127.0.0.1", 0, TOKEN, actions, self.audit_path, handlers)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def call(self, action, data):
        request = urllib.request.Request(f"{self.base}/v1/actions/{action}", data=data, method="POST")
        request.add_header("Authorization", f"Bearer {TOKEN}")
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                return response.status, json.loads(response.read())
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read())

    def test_an_implemented_action_answers_its_result_and_the_audit_line_has_no_body(self):
        # Arrange and Act
        status, body = self.call("db-outage", b'{"op": "stop", "secret": "do-not-log"}')

        # Assert
        self.assertEqual((status, body), (200, {"result": {"echo": {"op": "stop", "secret": "do-not-log"}}}))
        self.assertNotIn("do-not-log", self.audit_path.read_text(encoding="utf-8"))

    def test_a_command_that_hangs_is_an_audited_error_not_a_dropped_connection(self):
        # Arrange
        def hang(params):
            raise subprocess.TimeoutExpired("docker", 1)

        self.server.handlers["db-outage"] = hang

        # Act
        status, body = self.call("db-outage", b"{}")

        # Assert
        self.assertEqual((status, body["error"]["code"]), (500, "action_failed"))
        self.assertEqual(json.loads(self.audit_path.read_text(encoding="utf-8").splitlines()[-1])["status"], 500)

    def test_a_body_that_is_not_a_json_object_is_a_bad_request(self):
        # Arrange and Act
        status, body = self.call("db-outage", b"[1]")

        # Assert
        self.assertEqual((status, body["error"]["code"]), (400, "bad_request"))

    def test_an_action_without_a_handler_is_still_not_implemented(self):
        # Arrange and Act
        status, body = self.call("bit-flip", b"{}")

        # Assert
        self.assertEqual((status, body["error"]["code"]), (501, "not_implemented"))


if __name__ == "__main__":
    unittest.main()

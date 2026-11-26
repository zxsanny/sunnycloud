"""Unit tests of the e2e-ctl actions the worker suite (harness H2) adds: the worker's ops port, a decoder's process
listing, the api restart with a worker token rotation and the compose files of a harness profile. Fake docker, no
docker needed."""
import json
import tempfile
import unittest
from pathlib import Path

import ctl_actions

OLD_TOKEN = "old-worker-token-value-0123456789"
TOP_OUTPUT = (
    "PID                 COMMAND             COMMAND\n"
    "4101                docker-init         /sbin/docker-init -- decoder-runner\n"
    "4170                decoder-runner      decoder-runner\n"
    "4222                ffmpeg              ffmpeg -nostdin -i /scratch-general/j1/in/input.mov\n"
)


class FakeDocker:
    def __init__(self, top=TOP_OUTPUT):
        self.calls = []
        self.top = top
        self.services = "api\nworker\ndecoder-interactive\ndecoder-general\nfd-interactive\nfd-general\n"

    def __call__(self, args):
        self.calls.append(args)
        if "config" in args:
            return 0, self.services, ""
        if "ps" in args:
            return 0, "container-1\n", ""
        if "top" in args:
            return 0, self.top, ""
        if "inspect" in args and "Networks" in args[args.index("-f") + 1]:
            return 0, "172.30.0.9\n", ""
        if args[0] == "bash":
            return 0, "usr/local/bin/sunnycloud-worker\nusr/bin/\nopt/media/bin/ffmpeg\n", ""
        if "inspect" in args and "{{json .Mounts}}" in args:
            return 0, json.dumps([{"Destination": "/run/secrets/worker_api_token"},
                                  {"Destination": "/run/decoder-interactive"}]) + "\n", ""
        if "inspect" in args and "{{json .Config.Env}}" in args:
            return 0, json.dumps(["PATH=/usr/bin", "SUNNYCLOUD_JOB_LEASE=6s", "SUNNYCLOUD_WORKER_STATUS_INTERVAL=1s",
                                  "SUNNYCLOUD_WORKER_API_TOKEN=must-never-be-returned"]) + "\n", ""
        if "inspect" in args:
            return 0, "healthy\n", ""
        return 0, "", ""


class WorkerActionsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.secrets = Path(self.tmp.name)
        (self.secrets / "worker_api_token").write_text(OLD_TOKEN + "\n", encoding="utf-8")
        (self.secrets / "worker_api_token_next").write_text("", encoding="utf-8")
        self.docker = FakeDocker()
        self.requests = []
        self.actions = ctl_actions.Actions("proj", ["/x/compose.test.yml", "/x/compose.h2.yml"], self.tmp.name,
                                           run=self.docker, request=self.fake_request, sleep=lambda _: None)

    def tearDown(self):
        self.tmp.cleanup()

    def fake_request(self, address, method, path, headers, body):
        self.requests.append((address, method, path, headers, body))
        return 200, {"content-type": "text/plain"}, b"worker_selfcheck_ok 1\n"

    def token(self, name):
        return (self.secrets / name).read_text(encoding="utf-8").strip()

    def test_compose_commands_carry_every_compose_file_of_the_profile_in_order(self):
        # Act
        self.actions.process_kill({"service": "worker", "op": "start"})

        # Assert
        self.assertEqual(self.docker.calls[0][:8], ["docker", "compose", "-p", "proj", "-f", "/x/compose.test.yml",
                                                    "-f", "/x/compose.h2.yml"])

    def test_internal_http_reaches_the_workers_ops_port_on_its_backend_address(self):
        # Act
        result = self.actions.internal_http({"service": "worker", "port": 9090, "path": "/metrics"})

        # Assert
        self.assertEqual(self.requests, [(("172.30.0.9", 9090), "GET", "/metrics", {}, None)])
        self.assertEqual((result["status"], result["body"]), (200, "worker_selfcheck_ok 1\n"))

    def test_internal_http_refuses_a_worker_port_other_than_ops_and_the_worker_bearer_for_the_worker(self):
        # Arrange
        refused = [{"service": "worker", "port": 8081, "path": "/"}, {"service": "worker", "port": 9091, "path": "/"},
                   {"service": "worker", "port": 9090, "path": "/readyz", "auth": "worker"},
                   {"service": "decoder-general", "port": 9090, "path": "/"}]

        # Act
        statuses = []
        for params in refused:
            with self.assertRaises(ctl_actions.ActionError) as raised:
                self.actions.internal_http(params)
            statuses.append(raised.exception.status)

        # Assert
        self.assertEqual(statuses, [400] * len(refused))
        self.assertEqual((self.requests, self.docker.calls), ([], []))

    def test_inspect_processes_lists_a_decoders_processes_from_the_host(self):
        # Act
        result = self.actions.inspect({"processes": "decoder-general"})

        # Assert
        self.assertEqual([p["comm"] for p in result["processes"]], ["docker-init", "decoder-runner", "ffmpeg"])
        self.assertEqual(result["processes"][2], {"pid": 4222, "comm": "ffmpeg",
                                                  "args": "ffmpeg -nostdin -i /scratch-general/j1/in/input.mov"})
        self.assertEqual(self.docker.calls[-1], ["docker", "top", "container-1", "-o", "pid,comm,args"])

    def test_inspect_processes_refuses_a_service_that_is_not_a_decoder(self):
        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.inspect({"processes": "worker"})

        # Assert
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.docker.calls, [])

    def test_inspect_processes_reports_an_unreadable_listing_as_an_error(self):
        # Arrange
        self.docker.top = "PID COMMAND COMMAND\nnot-a-pid x y\n"

        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.inspect({"processes": "decoder-interactive"})

        # Assert
        self.assertEqual((raised.exception.status, raised.exception.code), (500, "top_failed"))

    def test_inspect_settings_returns_only_the_timing_settings_of_the_container(self):
        # Act
        result = self.actions.inspect({"settings": "worker"})

        # Assert
        self.assertEqual(result["settings"]["SUNNYCLOUD_JOB_LEASE"], "6s")
        self.assertEqual(result["settings"]["SUNNYCLOUD_WORKER_STATUS_INTERVAL"], "1s")
        self.assertIsNone(result["settings"]["SUNNYCLOUD_DECODER_CANCEL_GRACE"])
        self.assertEqual(len(result["settings"]), 7)
        self.assertNotIn("must-never-be-returned", json.dumps(result))

    def test_inspect_settings_refuses_a_service_other_than_the_api_and_the_worker(self):
        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.inspect({"settings": "db"})

        # Assert
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.docker.calls, [])

    def test_restart_with_next_adds_a_next_token_keeps_the_current_one_and_waits_for_a_healthy_api(self):
        # Act
        result = self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "next"})

        # Assert
        self.assertEqual(self.token("worker_api_token"), OLD_TOKEN)
        self.assertGreaterEqual(len(self.token("worker_api_token_next")), 32)
        self.assertEqual(self.docker.calls[0][-2:], ["restart", "api"])
        self.assertIn("{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}", self.docker.calls[-1])
        self.assertEqual(result, {"service": "api", "op": "restart", "worker_token": "next"})

    def test_restart_with_drop_old_promotes_the_next_token_and_empties_the_next_file(self):
        # Arrange
        self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "next"})
        following = self.token("worker_api_token_next")

        # Act
        self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "drop-old"})

        # Assert
        self.assertEqual((self.token("worker_api_token"), self.token("worker_api_token_next")), (following, ""))

    def test_restart_with_drop_old_without_a_next_token_replaces_the_token_with_a_fresh_one(self):
        # Act
        result = self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "drop-old"})

        # Assert
        self.assertNotEqual(self.token("worker_api_token"), OLD_TOKEN)
        self.assertGreaterEqual(len(self.token("worker_api_token")), 32)
        self.assertNotIn(self.token("worker_api_token"), json.dumps(result))

    def test_a_rotation_rewrites_the_secret_files_in_place(self):
        # Arrange: the api container holds a bind mount of each file, so a replaced inode would not reach it.
        inodes = [(self.secrets / n).stat().st_ino for n in ("worker_api_token", "worker_api_token_next")]

        # Act
        self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "next"})
        self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "drop-old"})

        # Assert
        self.assertEqual([(self.secrets / n).stat().st_ino for n in ("worker_api_token", "worker_api_token_next")], inodes)

    def test_restart_refuses_other_services_unknown_stages_and_a_token_on_other_ops_without_side_effects(self):
        # Arrange
        refused = [{"service": "worker", "op": "restart"}, {"service": "api", "op": "restart", "worker_token": "swap"},
                   {"service": "api", "op": "kill", "worker_token": "next"},
                   {"service": "worker", "op": "start", "worker_token": "drop-old"},
                   {"service": "decoder-general", "op": "pause", "worker_token": "next"}]

        # Act
        statuses = []
        for params in refused:
            with self.assertRaises(ctl_actions.ActionError) as raised:
                self.actions.process_kill(params)
            statuses.append(raised.exception.status)

        # Assert
        self.assertEqual(statuses, [400] * len(refused))
        self.assertEqual(self.docker.calls, [])
        self.assertEqual((self.token("worker_api_token"), self.token("worker_api_token_next")), (OLD_TOKEN, ""))

    def test_a_rotation_on_a_stack_without_the_next_secret_is_refused_before_any_restart(self):
        # Arrange
        (self.secrets / "worker_api_token_next").unlink()

        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.process_kill({"service": "api", "op": "restart", "worker_token": "next"})

        # Assert
        self.assertEqual((raised.exception.status, raised.exception.code), (409, "no_next_token_secret"))
        self.assertEqual((self.docker.calls, self.token("worker_api_token")), ([], OLD_TOKEN))


    def test_fake_stops_the_decoder_and_starts_a_new_fd_container_on_its_lane(self):
        # Act
        result = self.actions.process_kill({"service": "decoder-general", "op": "fake"})

        # Assert
        commands = [c[c.index("--profile"):] for c in self.docker.calls]
        self.assertEqual(commands[1:], [["--profile", "core", "--profile", "fd", "stop", "decoder-general"],
                                        ["--profile", "core", "--profile", "fd", "up", "-d", "--no-deps", "--force-recreate",
                                         "fd-general"]])
        self.assertEqual(result, {"service": "decoder-general", "op": "fake"})

    def test_real_stops_fd_and_starts_the_decoder_until_it_is_healthy(self):
        # Act
        self.actions.process_kill({"service": "decoder-interactive", "op": "real"})

        # Assert
        commands = [c[c.index("--profile"):] for c in self.docker.calls if "compose" in c]
        self.assertEqual(commands[1:3], [["--profile", "core", "--profile", "fd", "stop", "fd-interactive"],
                                         ["--profile", "core", "--profile", "fd", "start", "decoder-interactive"]])
        self.assertIn("{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}", self.docker.calls[-1])

    def test_a_swap_on_a_stack_without_fd_is_refused_before_the_decoder_is_stopped(self):
        # Arrange
        self.docker.services = "api\nworker\ndecoder-interactive\ndecoder-general\n"

        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.process_kill({"service": "decoder-interactive", "op": "fake"})

        # Assert
        self.assertEqual((raised.exception.status, raised.exception.code), (409, "no_fake_decoder"))
        self.assertEqual(len(self.docker.calls), 1)

    def test_a_swap_is_refused_for_a_service_that_is_not_a_decoder(self):
        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.process_kill({"service": "worker", "op": "fake"})

        # Assert
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.docker.calls, [])


    def test_scratch_fill_writes_leftovers_as_the_supervisor_into_the_lanes_volume(self):
        # Act
        result = self.actions.scratch_fill({"lane": "general", "trees": 3})

        # Assert
        run = self.docker.calls[-1]
        self.assertEqual(run[:7], ["docker", "run", "--rm", "--network", "none", "--user", "10002:10003"])
        self.assertIn("proj_scratch_general:/s", run)
        self.assertEqual(run[-1], "3")
        self.assertEqual(result, {"lane": "general", "trees": 3, "stray": 1})

    def test_scratch_fill_refuses_an_unknown_lane_and_a_count_out_of_range_without_a_container(self):
        # Arrange
        refused = [{"lane": "blobs"}, {"lane": "general", "trees": 0}, {"lane": "general", "trees": 11},
                   {"lane": "general", "trees": True}, {"lane": "general", "trees": "2"}]

        # Act
        statuses = []
        for params in refused:
            with self.assertRaises(ctl_actions.ActionError) as raised:
                self.actions.scratch_fill(params)
            statuses.append(raised.exception.status)

        # Assert
        self.assertEqual(statuses, [400] * len(refused))
        self.assertEqual(self.docker.calls, [])

    def test_inspect_container_reports_names_secrets_mounts_and_media_tools_never_a_value(self):
        # Act
        facts = self.actions.inspect({"container": "worker"})

        # Assert
        self.assertIn("SUNNYCLOUD_WORKER_API_TOKEN", facts["env_names"])
        self.assertNotIn("must-never-be-returned", json.dumps(facts))
        self.assertEqual(facts["secrets"], ["worker_api_token"])
        self.assertEqual(facts["mounts"], ["/run/decoder-interactive", "/run/secrets/worker_api_token"])
        self.assertEqual(facts["media_tools"], ["opt/media/bin/ffmpeg"])

    def test_inspect_container_refuses_a_service_it_does_not_list(self):
        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.inspect({"container": "api"})

        # Assert
        self.assertEqual(raised.exception.status, 400)
        self.assertEqual(self.docker.calls, [])


    def test_db_probe_worker_status_latest_reads_the_newest_report_and_minus_one_before_any(self):
        # Arrange
        answers = iter(["0\n", "-1\n"])
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=lambda args: (0, next(answers), ""))

        # Act
        latest, none = (actions.db_probe({"query": "worker_status_latest"}) for _ in range(2))

        # Assert
        self.assertEqual((latest, none), ({"decoder_ok": 0}, {"decoder_ok": -1}))


if __name__ == "__main__":
    unittest.main()

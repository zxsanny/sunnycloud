"""Unit tests of the e2e-ctl input faults of the worker suite (IT-09-7, IT-09-6 (i)): `bit-flip`, `job-row-edit`,
`worker-variant` and the `job_rows` probe. Fake docker, no docker needed."""
import tempfile
import unittest

import ctl_actions
import ctl_worker

BLOB = "3f2a9c4e-8b1d-4e5f-9a6b-7c8d9e0f1a2b"
JOB = "0193b2a4-5c6d-4e7f-8a9b-0c1d2e3f4a5b"
INPUT_LINE = '{"msg":"request answered","route":"/api/v1/worker-jobs/{job_id}/input","status":500}\n'
LOG = '{"msg":"listening"}\n' + INPUT_LINE + '{"msg":"request answered","route":"/api/v1/worker-jobs/claims","status":403}\n'


class FakeDocker:
    def __init__(self, flip="64 65", psql=JOB + "\nUPDATE 1\n"):
        self.calls = []
        self.flip = flip
        self.psql = psql

    def __call__(self, args):
        self.calls.append(args)
        if args[:2] == ["docker", "run"]:
            return 0, self.flip, ""
        if "psql" in args:
            return 0, self.psql, ""
        if "logs" in args:
            return 0, LOG, ""
        if "ps" in args:
            return 0, "container-1\n", ""
        if "{{.Config.Image}}" in args:
            return 0, "postgres:18-trixie\n", ""
        if "inspect" in args:
            return 0, "healthy\n", ""
        return 0, "", ""


class StartDocker:
    """Answers the start-state reads from `states` in order (the last one repeats) and counts the starts."""

    def __init__(self, states):
        self.states = list(states)
        self.starts = 0

    def __call__(self, args):
        if "ps" in args:
            return 0, "container-1\n", ""
        if "{{.State.Status}} {{.State.StartedAt}}" in args:
            return 0, (self.states.pop(0) if len(self.states) > 1 else self.states[0]) + "\n", ""
        if args[-2:] == ["start", "worker"]:
            self.starts += 1
        return 0, "", ""


class CheckedStartTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.tmp.cleanup()

    def start(self, states):
        docker = StartDocker(states)
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=docker, sleep=lambda _: None)
        return actions, docker

    def test_a_start_that_left_the_old_start_time_is_tried_once_more(self):
        # Arrange: the first start changes nothing, the second one starts the container.
        old, new = "exited 2026-11-27T08:23:02Z", "running 2026-11-27T08:23:40Z"
        actions, docker = self.start([old] * (1 + ctl_worker.START_CHECKS) + [new])

        # Act
        actions.process_kill({"service": "worker", "op": "start"})

        # Assert
        self.assertEqual(docker.starts, 2)

    def test_two_starts_that_leave_the_old_start_time_are_an_error(self):
        # Arrange
        actions, docker = self.start(["exited 2026-11-27T08:23:02Z"])

        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            actions.process_kill({"service": "worker", "op": "start"})

        # Assert
        self.assertEqual((raised.exception.status, raised.exception.code, docker.starts), (500, "start_failed", 2))

    def test_a_container_that_already_runs_is_not_started(self):
        # Arrange
        actions, docker = self.start(["running 2026-11-27T09:08:18Z"])

        # Act
        actions.process_kill({"service": "worker", "op": "start"})

        # Assert
        self.assertEqual(docker.starts, 0)

    def test_a_container_that_started_and_exited_is_not_started_again(self):
        # Arrange: a new start time with status exited is the service's own exit.
        actions, docker = self.start(["exited 2026-11-27T08:23:02Z", "exited 2026-11-27T08:23:41Z"])

        # Act
        actions.process_kill({"service": "worker", "op": "start"})

        # Assert
        self.assertEqual(docker.starts, 1)


class StopTest(unittest.TestCase):
    def test_stop_sends_sigterm_through_compose_and_reports_the_time_and_the_exit_code(self):
        # Arrange
        calls = []

        def docker(args):
            calls.append(args)
            if "ps" in args:
                return 0, "container-1\n", ""
            return (0, "0\n", "") if "{{.State.ExitCode}}" in args else (0, "", "")

        with tempfile.TemporaryDirectory() as tmp:
            actions = ctl_actions.Actions("proj", "/x/compose.yml", tmp, run=docker)

            # Act
            result = actions.process_kill({"service": "worker", "op": "stop"})
            with self.assertRaises(ctl_actions.ActionError) as refused:
                actions.process_kill({"service": "api", "op": "stop"})

        # Assert
        self.assertEqual(calls[0][-4:], ["stop", "-t", str(ctl_worker.STOP_TIMEOUT_SECONDS), "worker"])
        self.assertEqual((result["exit_code"], result["op"], refused.exception.status), (0, "stop", 400))
        self.assertGreaterEqual(result["seconds"], 0)


class InputFaultsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.docker = FakeDocker()
        self.actions = ctl_actions.Actions("proj", ["/x/compose.test.yml", "/x/compose.h2.yml"], self.tmp.name,
                                           run=self.docker, sleep=lambda _: None)

    def tearDown(self):
        self.tmp.cleanup()

    def refused(self, call, params):
        with self.assertRaises(ctl_actions.ActionError) as raised:
            call(params)
        return raised.exception.status

    def test_bit_flip_rewrites_the_first_chunk_byte_of_the_blob_file_in_the_blob_volume(self):
        # Act
        result = self.actions.bit_flip({"blob_id": BLOB.upper()})

        # Assert
        run = self.docker.calls[-1]
        self.assertEqual(run[:9], ["docker", "run", "--rm", "--network", "none", "--user", "0:0", "-v", "proj_blobs:/b"])
        self.assertEqual(run[-3:], ["sh", f"/b/blobs/3f/2a/{BLOB}", "64"])
        self.assertEqual(result, {"blob_id": BLOB, "offset": 64, "before": 64, "after": 65})

    def test_bit_flip_reports_an_unreadable_answer_of_the_helper_as_an_error(self):
        # Arrange
        self.docker.flip = "64"

        # Act
        with self.assertRaises(ctl_actions.ActionError) as raised:
            self.actions.bit_flip({"blob_id": BLOB})

        # Assert
        self.assertEqual((raised.exception.status, raised.exception.code), (500, "flip_failed"))

    def test_job_row_edit_runs_the_one_fixed_update_on_the_canonical_job_id(self):
        # Act
        result = self.actions.job_row_edit({"job": JOB})

        # Assert
        statement = self.docker.calls[-1][-1]
        self.assertEqual(statement, ctl_worker.JOB_HASH_EDIT.format(job=JOB))
        self.assertIn("status = 'queued'", statement)
        self.assertEqual(result, {"job": JOB, "edited": 1})

    def test_job_row_edit_of_a_job_that_is_not_queued_edits_nothing(self):
        # Arrange
        self.docker.psql = "UPDATE 0\n"

        # Act
        result = self.actions.job_row_edit({"job": JOB})

        # Assert
        self.assertEqual(result["edited"], 0)

    def test_an_id_that_is_not_a_uuid_is_refused_before_any_command(self):
        # Arrange
        hostile = [(self.actions.job_row_edit, {"job": "x' OR '1'='1"}), (self.actions.job_row_edit, {}),
                   (self.actions.bit_flip, {"blob_id": "../../etc/passwd"}), (self.actions.bit_flip, {"blob_id": 7})]

        # Act
        statuses = [self.refused(call, params) for call, params in hostile]

        # Assert
        self.assertEqual((statuses, self.docker.calls), ([400] * len(hostile), []))

    def test_worker_variant_recreates_the_worker_with_the_variant_file_after_the_stacks_files(self):
        # Act
        result = self.actions.worker_variant({"variant": "photo-limit-1mib"})

        # Assert
        up = self.docker.calls[0]
        variant_file = str(ctl_worker.WORKER_VARIANTS_DIR / "photo-limit-1mib.yml")
        self.assertEqual(up[4:10], ["-f", "/x/compose.test.yml", "-f", "/x/compose.h2.yml", "-f", variant_file])
        self.assertEqual(up[-6:], ["up", "-d", "--no-deps", "--no-build", "--force-recreate", "worker"])
        self.assertEqual(result, {"variant": "photo-limit-1mib"})

    def test_worker_variant_default_recreates_the_worker_from_the_stacks_files_only(self):
        # Act
        self.actions.worker_variant({"variant": "default"})

        # Assert
        up = self.docker.calls[0]
        self.assertEqual([arg for arg in up if arg.endswith(".yml")], ["/x/compose.test.yml", "/x/compose.h2.yml"])

    def test_worker_variant_refuses_a_variant_it_does_not_list(self):
        # Act
        status = self.refused(self.actions.worker_variant, {"variant": "../compose.test"})

        # Assert
        self.assertEqual((status, self.docker.calls), (400, []))

    def test_every_listed_variant_but_default_has_its_override_file(self):
        # Act
        missing = [name for name in ctl_worker.WORKER_VARIANTS - {"default"}
                   if not (ctl_worker.WORKER_VARIANTS_DIR / f"{name}.yml").is_file()]

        # Assert
        self.assertEqual(missing, [])

    def test_inspect_logs_lines_with_keeps_only_the_lines_holding_the_text(self):
        # Act
        result = self.actions.inspect({"logs": "api", "lines_with": "/worker-jobs/{job_id}/input"})

        # Assert
        self.assertEqual(result, {"log_tail": INPUT_LINE})

    def test_inspect_logs_refuses_an_empty_or_non_text_filter(self):
        # Act
        statuses = [self.refused(self.actions.inspect, {"logs": "api", "lines_with": needle}) for needle in ("", 5)]

        # Assert
        self.assertEqual((statuses, self.docker.calls), ([400, 400], []))

    def test_db_probe_job_rows_lists_each_job_with_its_state_and_blob(self):
        # Arrange
        rows = f"{JOB}:stills:queued:1:HashMismatch:{BLOB},j2:poster:dead:1:TooLarge:\n"
        actions = ctl_actions.Actions("proj", "/x/compose.yml", self.tmp.name, run=lambda args: (0, rows, ""))

        # Act
        result = actions.db_probe({"query": "job_rows"})

        # Assert
        self.assertEqual(result, {"values": [f"{JOB}:stills:queued:1:HashMismatch:{BLOB}", "j2:poster:dead:1:TooLarge:"]})


if __name__ == "__main__":
    unittest.main()

"""Unit tests of the e2e-ctl skeleton; no docker needed: python3 -m unittest -v (from this directory)."""
import json
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

import e2e_ctl

TOKEN = "unit-test-token"
ACTIONS_FILE = Path(__file__).with_name("actions.json")


class CtlStubTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.audit_path = Path(self.tmp.name) / "ctl-audit.log"
        self.actions = e2e_ctl.load_actions(ACTIONS_FILE)
        self.server = e2e_ctl.build_server("127.0.0.1", 0, TOKEN, self.actions, self.audit_path)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def call(self, path, token=TOKEN, method="POST", data=b"{}"):
        request = urllib.request.Request(self.base + path, data=data, method=method)
        if token is not None:
            request.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                return response.status, json.loads(response.read())
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read())

    def audit(self):
        if not self.audit_path.exists():
            return []
        return [json.loads(line) for line in self.audit_path.read_text(encoding="utf-8").splitlines()]

    def test_every_documented_action_is_listed(self):
        self.assertEqual(len(self.actions), 22)

    def test_every_action_with_token_returns_not_implemented_and_one_audit_line(self):
        for index, action in enumerate(sorted(self.actions), start=1):
            with self.subTest(action=action):
                status, body = self.call(f"/v1/actions/{action}")
                self.assertEqual(status, 501)
                self.assertEqual(body["error"]["code"], "not_implemented")
                self.assertEqual(body["error"]["action"], action)
                self.assertEqual(len(self.audit()), index)
                self.assertEqual(self.audit()[-1]["action"], action)

    def test_call_without_token_is_refused_and_audited(self):
        status, body = self.call("/v1/actions/process-kill", token=None)
        self.assertEqual((status, body["error"]["code"]), (401, "unauthorized"))
        self.assertEqual([(l["auth"], l["status"], l["action"]) for l in self.audit()], [("missing", 401, "process-kill")])

    def test_call_with_wrong_token_is_refused_and_audited(self):
        status, body = self.call("/v1/actions/process-kill", token="wrong")
        self.assertEqual((status, body["error"]["code"]), (401, "unauthorized"))
        self.assertEqual([(l["auth"], l["status"]) for l in self.audit()], [("invalid", 401)])

    def test_unknown_action_is_a_typed_error_and_audited(self):
        status, body = self.call("/v1/actions/not-an-action")
        self.assertEqual((status, body["error"]["code"]), (404, "unknown_action"))
        self.assertEqual(len(self.audit()), 1)

    def test_wrong_method_is_a_typed_error_and_audited(self):
        status, body = self.call("/v1/actions/process-kill", method="GET", data=None)
        self.assertEqual((status, body["error"]["code"]), (405, "method_not_allowed"))
        self.assertEqual(len(self.audit()), 1)

    def test_unknown_path_is_a_typed_error_and_audited(self):
        status, body = self.call("/elsewhere")
        self.assertEqual((status, body["error"]["code"]), (404, "not_found"))
        self.assertEqual(len(self.audit()), 1)

    def test_audit_never_contains_the_token(self):
        self.call("/v1/actions/process-kill")
        self.call("/v1/actions/process-kill", token="wrong-secret-value")
        self.assertNotIn("wrong-secret-value", self.audit_path.read_text(encoding="utf-8"))
        self.assertNotIn(TOKEN, self.audit_path.read_text(encoding="utf-8"))

    def test_wildcard_bind_is_refused(self):
        for address in ("0.0.0.0", "::", ""):
            with self.subTest(address=address), self.assertRaises(ValueError):
                e2e_ctl.build_server(address, 0, TOKEN, self.actions, self.audit_path)

    def test_empty_token_file_is_refused(self):
        token_file = Path(self.tmp.name) / "token"
        token_file.write_text("\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            e2e_ctl.load_token(token_file)


if __name__ == "__main__":
    unittest.main(verbosity=2)

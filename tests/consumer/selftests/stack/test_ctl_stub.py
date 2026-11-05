import json
import urllib.error
import urllib.request


def call(base, action, token):
    request = urllib.request.Request(f"{base}/v1/actions/{action}", data=b"{}", method="POST")
    if token is not None:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def audit_lines(path):
    if not path.exists():  # the stub creates the log with its first call
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


def test_every_action_with_the_token_returns_not_implemented_and_one_audit_line_each(ctl):
    # Arrange
    before = len(audit_lines(ctl["audit"]))

    # Act
    answers = {action: call(ctl["url"], action, ctl["token"]) for action in ctl["actions"]}
    after = audit_lines(ctl["audit"])

    # Assert
    assert len(ctl["actions"]) == 22
    assert {a: (s, b["error"]["code"]) for a, (s, b) in answers.items()} == {a: (501, "not_implemented") for a in ctl["actions"]}
    assert [line["action"] for line in after[before:]] == ctl["actions"]
    assert all(line["auth"] == "ok" and line["status"] == 501 for line in after[before:])


def test_call_without_the_token_is_refused_and_audited(ctl):
    # Arrange
    before = len(audit_lines(ctl["audit"]))

    # Act
    status, body = call(ctl["url"], ctl["actions"][0], None)
    new_lines = audit_lines(ctl["audit"])[before:]

    # Assert
    assert (status, body["error"]["code"]) == (401, "unauthorized")
    assert [(line["auth"], line["status"], line["action"]) for line in new_lines] == [("missing", 401, ctl["actions"][0])]


def test_audit_log_never_holds_the_token(ctl):
    # Arrange
    call(ctl["url"], ctl["actions"][0], ctl["token"])

    # Act
    text = ctl["audit"].read_text(encoding="utf-8")

    # Assert
    assert ctl["token"] not in text

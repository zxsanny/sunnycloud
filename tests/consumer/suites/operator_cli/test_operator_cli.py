"""AZ-3815 (E-22/c17) on the stack: the operator CLI with real components. AT-12-1 (the install check, AC-1), ST-12-1
(secrets never leave the secret channel, AC-5), ST-12-2 (destructive commands need confirmation, AC-6), ST-12-3 (no
listener of its own, AC-7; the crate grep is the `cli-boundary` gate of scripts/ci/check-db-gates.sh) and ST-12-4 (the
api role cannot change the schema, AC-8). PT-12-1 and PT-12-2 (AC-3, AC-4) are timing tests of the reference server:
NOT-RUN(HW-REF) here. AT-12-2 (the restore drill, AC-2) is its own module.

The commands run in the api container through the ctl action `exec-operator` (stdin empty and not a TTY), `migrate`
in a one-off container of the `migrate` service, the only one holding the migrator secret. The stack carries the api
test image (feature `test-clock`); the release-image leg of AT-12-1 waits for the release image in the stack (AZ-3835).

Each test starts from the golden snapshot (no accounts, fixture `fresh_install`).
"""
import json

import pytest

from stackkit import Browser, operator_token, wait_worker_ok

ADMIN_ID = "/api/v1/accounts/me"
CALLBACK = "https://sunnycloud.test/api/v1/auth/providers/oidc/callback"
EXIT_USAGE = 3


def run(ctl, *args, **extra):
    return ctl.call("exec-operator", args=list(args), **extra)


def findings(result):
    """The findings of a `doctor --json` run: `{check, level, message, remedy}` each."""
    envelope = json.loads(result["stdout"])
    return envelope["data"]["findings"]


def first_admin(ctl):
    """Signs alice in with the setup token; her browser and account id."""
    browser = Browser()
    browser.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
    status, account = browser.api(ADMIN_ID)
    assert status == 200, account
    return browser, account["id"]


def test_at_12_1_pre_flight_prints_the_redirect_uri_then_the_first_admin_and_doctor_exit_0(fresh_install):
    # Arrange
    ctl = fresh_install

    # Act
    pre_flight = run(ctl, "doctor", "--pre-flight", "--json")
    browser, _ = first_admin(ctl)
    wait_worker_ok(ctl)
    doctor = run(ctl, "doctor", "--json")

    # Assert
    assert pre_flight["exit_code"] == 0, pre_flight
    pre = findings(pre_flight)
    assert {f["level"] for f in pre} == {"ok"}, pre
    assert {"identity_providers", "master_key"} <= {f["check"] for f in pre}, pre
    assert any(CALLBACK in f["message"] for f in pre if f["check"] == "identity_providers"), pre
    assert browser.signed_in()
    assert doctor["exit_code"] == 0, [f for f in findings(doctor) if f["level"] != "ok"] or doctor


def test_st_12_1_each_token_appears_once_on_its_creating_stdout_and_no_key_appears_anywhere(fresh_install):
    # Arrange
    ctl = fresh_install
    wait_worker_ok(ctl)
    setup = run(ctl, "admin", "setup-token")
    browser = Browser()

    # Act
    tokens = [line[len("token: "):] for line in setup["stdout"].splitlines() if line.startswith("token: ")]
    browser.sign_in("alice", setup_token=tokens[0])
    _, account = browser.api(ADMIN_ID)
    creating = [setup,
                run(ctl, "admin", "create", "--json"),
                run(ctl, "admin", "invite", "--admin", account["id"], "create", "--note", "st-12-1"),
                run(ctl, "admin", "reset", account["id"], "--json")]
    for result in creating[1:]:
        if result["stdout"].lstrip().startswith("{"):
            tokens.append(json.loads(result["stdout"])["data"]["token"])
        else:
            tokens += [line[len("token: "):] for line in result["stdout"].splitlines() if line.startswith("token: ")]
    others = [run(ctl, "doctor", "--json"), run(ctl, "admin", "user", "list", "--json"),
              run(ctl, "admin", "invite", "--admin", account["id"], "list", "--json"), run(ctl, "key-check", "--json")]
    shown = creating + others
    outputs = "\n".join(r["stdout"] + "\n" + r["stderr"] for r in shown)
    scan = ctl.call("inspect", secret_scan={"tokens": tokens, "outputs": outputs})["counts"]

    # Assert
    assert all(r["exit_code"] == 0 for r in shown), [r for r in shown if r["exit_code"] != 0]
    assert len(tokens) == len(creating), tokens
    for index, token in enumerate(tokens):
        on_stdout = [i for i, r in enumerate(shown) if token in r["stdout"]]
        assert on_stdout == [index], f"token {index} is on the stdout of commands {on_stdout}"
        assert not any(token in r["stderr"] for r in shown), f"token {index} is on a stderr"
        assert scan["outputs"][f"token_{index}"] == 1
        for place in ("logs", "audit", "ps"):
            assert scan[place][f"token_{index}"] == 0, f"token {index} is in the {place}"
    for place, counts in scan.items():
        assert {label: n for label, n in counts.items() if label.startswith("master_key") and n} == {}, place


def test_st_12_2_destructive_commands_without_yes_exit_3_and_change_nothing(fresh_install):
    # Arrange
    ctl = fresh_install
    _, account_id = first_admin(ctl)
    before = (ctl.call("db-probe", query="table_rows"), ctl.call("db-probe", query="schema_and_epochs"))
    commands = [(("admin", "user", "delete", account_id), {}),
                (("admin", "rewrap", "--from-key-id", "1"), {}),
                (("admin", "epoch-bump"), {}),
                (("migrate", "revert"), {"service": "migrate"})]

    # Act
    results = [run(ctl, *args, **extra) for args, extra in commands]
    after = (ctl.call("db-probe", query="table_rows"), ctl.call("db-probe", query="schema_and_epochs"))

    # Assert
    for (args, _), result in zip(commands, results):
        assert (result["exit_code"], result["stdout"]) == (EXIT_USAGE, ""), (args, result)
    assert after == before


def test_st_12_3_doctor_and_a_scrub_run_open_no_listening_socket(fresh_install):
    # Arrange
    ctl = fresh_install
    first_admin(ctl)
    wait_worker_ok(ctl)

    # Act
    doctor = run(ctl, "doctor", watch_listeners=True)
    scrub = run(ctl, "scrub", "run", "--full", watch_listeners=True)

    # Assert
    for name, result in (("doctor", doctor), ("scrub run", scrub)):
        assert result["exit_code"] in (0, 1), (name, result)
        assert result["listening_before"], f"{name}: the sampler saw no listener of serve at all"
        assert result["new_listening"] == [], f"{name} opened {result['new_listening']}"


def test_st_12_4_the_api_role_cannot_change_the_schema_and_holds_no_migrator_secret(fresh_install):
    # Arrange
    ctl = fresh_install

    # Act
    role = ctl.call("inspect", db_role="api")

    # Assert
    assert role["statements"]["drop_table"] == {"result": "denied", "sqlstate": "42501"}, role
    assert role["statements"]["create_table"]["result"] == "denied", role
    assert "db_api_url" in role["api_secrets"], role
    assert role["migrator_names"] == [], role


@pytest.mark.not_run("HW-REF", "PT-12-1 command latencies at 500 accounts and 100,000 files are timing results of the "
                                "reference server (environment.md, Where each tier runs); leftover 2026-11-14_ref-srv-timing-not-run.md")
def test_pt_12_1_command_latencies_at_scale():
    pass


@pytest.mark.not_run("HW-REF", "PT-12-2 scheduler overhead (10 idle minutes, CPU and query counts) is a reference-server "
                                "measurement; leftover 2026-11-14_ref-srv-timing-not-run.md")
def test_pt_12_2_scheduler_overhead_at_idle():
    pass

import json
import os
from pathlib import Path

import pytest


def required_env(name):
    value = os.environ.get(name)
    if not value:
        pytest.fail(f"harness misconfigured: {name} is not set (tests/run.sh sets it)", pytrace=False)
    return value


@pytest.fixture(scope="session")
def db_addr():
    return required_env("E2E_DB_ADDR")


@pytest.fixture(scope="session")
def ctl():
    token_file = Path(os.environ.get("E2E_CTL_TOKEN_FILE", "/run/secrets/ctl_token"))
    actions_file = Path(os.environ.get("E2E_CTL_ACTIONS_FILE", "/spec/e2e-ctl-actions.json"))
    return {
        "url": required_env("E2E_CTL_URL").rstrip("/"),
        "token": token_file.read_text(encoding="utf-8").strip(),
        "audit": Path(os.environ.get("E2E_RESULTS_DIR", "/results")) / "ctl-audit.log",
        "actions": json.loads(actions_file.read_text(encoding="utf-8"))["actions"],
    }

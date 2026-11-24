"""AZ-3664 AC-5 and NFT-SEC-04 on the stack: the sign-in limit per client address and what the audit log keeps.

The limit is 10 failed sign-in steps per client address per minute, fixed by AC-20; a refused request counts as one. The
test starts from the golden snapshot, so the api is new and its counters are empty.
"""
from stackkit import Browser, operator_token

LIMIT = 10
SECRET_KEYS = {"token", "code", "secret", "password", "email", "name", "refresh", "access", "invite", "setup_token"}


def test_the_eleventh_failed_step_in_a_minute_is_429_with_retry_after_and_is_audited_with_ids_only(fresh_install):
    # Arrange
    Browser().sign_in("alice", setup_token=operator_token(fresh_install, "admin", "setup-token"))
    attacker = Browser()

    # Act
    answers = [attacker.begin(return_to="https://evil.example/")[0] for _ in range(LIMIT)]
    status, headers, _ = attacker.request(
        "https://sunnycloud.test/api/v1/auth/login-attempts",
        "POST",
        {"provider": "oidc", "return_to": "https://evil.example/", "client_kind": "web"},
    )
    legit_status, _ = Browser().begin()
    events = fresh_install.call("db-probe", query="audit_events")
    keys = fresh_install.call("db-probe", query="audit_detail_keys")["values"]

    # Assert
    assert 429 not in answers and all(a >= 400 for a in answers), answers
    assert status == 429, status
    assert int(headers.get("Retry-After", "0")) >= 1, headers
    assert legit_status == 429, "the limit is per address: a valid request from the same address is refused too"
    assert events["abuse_trip"] >= 1, events
    assert events["sign_in"] >= 1, events
    assert not SECRET_KEYS & set(keys), f"the audit detail has keys that name a secret or a person: {keys}"

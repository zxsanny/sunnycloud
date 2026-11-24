"""AZ-3664 (flow F2) on the stack, the success paths: an invite sign-in creates a complete account (AC-1), the mobile
one-time code (AC-4) and one invite redeemed by two identities at once (AC-75). The refusals are in
`test_negative_callbacks.py` (AC-2), the session rules in `test_session_rules.py` (AC-3), the limiter and the audit
rows in `test_abuse_and_audit.py` (AC-5).
"""
import concurrent.futures
import urllib.parse

from stackkit import Browser, budget, operator_token, pkce_pair

ROUNDS = 20
MOBILE_RETURN = "/mobile-done"


def is_refused(location):
    return "error=" in location


def first_admin(ctl):
    admin = Browser()
    admin.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
    status, me = admin.api("/api/v1/accounts/me")
    assert status == 200, me
    return admin, me["id"]


def invite_for(ctl, admin_id):
    return operator_token(ctl, "admin", "invite", "--admin", admin_id, "create")


def rows(ctl):
    return ctl.call("db-probe", query="account_rows")


def code_of(location):
    query = urllib.parse.urlparse(location).query
    return urllib.parse.parse_qs(query)["code"][0]


def test_an_invite_sign_in_creates_one_complete_account(fresh_install):
    # Arrange
    _, admin_id = first_admin(fresh_install)
    invite = invite_for(fresh_install, admin_id)
    before = rows(fresh_install)
    newcomer = Browser()

    # Act
    location = newcomer.sign_in("bob", invite_token=invite)
    after = rows(fresh_install)
    status, me = newcomer.api("/api/v1/accounts/me")

    # Assert
    assert before == {"accounts": 1, "identities": 1, "quota": 1, "account_feeds": 1}
    assert not is_refused(location), location
    assert after == {key: count + 1 for key, count in before.items()}, after
    assert (status, me["role"]) == (200, "user"), me


def test_a_mobile_code_gives_session_tokens_once_and_only_to_the_verifier_holder(fresh_install):
    # Arrange
    first_admin(fresh_install)
    verifier, challenge = pkce_pair()
    _, other_challenge = pkce_pair()
    login = Browser()
    code = code_of(login.sign_in("alice", client_kind="android", return_to=MOBILE_RETURN, code_challenge=challenge))
    stolen = code_of(Browser().sign_in("alice", client_kind="android", return_to=MOBILE_RETURN, code_challenge=other_challenge))
    exchange = Browser()

    # Act
    first = exchange.api("/api/v1/auth/mobile-code-exchanges", "POST", {"code": code, "code_verifier": verifier})
    second = exchange.api("/api/v1/auth/mobile-code-exchanges", "POST", {"code": code, "code_verifier": verifier})
    wrong = exchange.api("/api/v1/auth/mobile-code-exchanges", "POST", {"code": stolen, "code_verifier": verifier})
    bearer = {"Authorization": f"Bearer {first[1].get('access_token', '')}"}
    me_status, me = exchange.api("/api/v1/accounts/me", headers=bearer)

    # Assert
    assert first[0] == 200 and {"access_token", "refresh_token"} <= set(first[1]), first
    assert second[0] != 200, f"a login code was exchanged twice: {second}"
    assert wrong[0] != 200, f"a code was exchanged with another app's verifier: {wrong}"
    assert (me_status, me["role"]) == (200, "admin"), me


def test_one_invite_redeemed_by_two_identities_at_once_makes_exactly_one_account(fresh_install):
    # Arrange
    _, admin_id = first_admin(fresh_install)
    failures = []

    for round_number in range(ROUNDS):
        budget.spend(1)  # the loser's failed step; see FailureBudget
        invite = invite_for(fresh_install, admin_id)
        before = rows(fresh_install)
        names = [f"race-a-{round_number}", f"race-b-{round_number}"]
        browsers = {name: Browser() for name in names}

        # Act
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures = {name: pool.submit(browsers[name].sign_in, name, None, invite) for name in names}
            locations = {name: future.result() for name, future in futures.items()}
        after = rows(fresh_install)

        # Assert
        winners = [name for name in names if not is_refused(locations[name]) and browsers[name].signed_in()]
        if len(winners) != 1 or after != {key: count + 1 for key, count in before.items()}:
            failures.append((round_number, locations, before, after))

    assert not failures, f"{len(failures)} of {ROUNDS} rounds broke the rule: {failures[:2]}"

"""AZ-3664 AC-3 on the stack: the session rules end to end, with the server's test clock. The refresh grace of the
stack is 5 s (`SUNNYCLOUD_REFRESH_REUSE_GRACE` in `tests/compose.test.yml`), so the clock moves by a little more than
that. Refreshes use the native client's route (`refresh-tokens`, a JSON body); the cookie rule uses the browser's.

Every test starts from the golden snapshot, which also gives the api a clock offset of zero.
"""
import concurrent.futures
import time

from stackkit import Browser, advance_clock, operator_token, pkce_pair

REUSE_GRACE_SECONDS = 5
REVOCATION_SECONDS = 5
PUBLIC_ORIGIN = "https://sunnycloud.test"
REFRESH = "/api/v1/auth/refresh-tokens"


def native_tokens(ctl):
    """The first admin signs in as a native client: `(access_token, refresh_token)` from the code exchange."""
    Browser().sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
    verifier, challenge = pkce_pair()
    location = Browser().sign_in("alice", client_kind="android", return_to="/mobile-done", code_challenge=challenge)
    code = location.split("code=", 1)[1].split("&", 1)[0]
    status, tokens = Browser().api("/api/v1/auth/mobile-code-exchanges", "POST", {"code": code, "code_verifier": verifier})
    assert status == 200, tokens
    return tokens["access_token"], tokens["refresh_token"]


def refresh(token):
    return Browser().api(REFRESH, "POST", {"refresh_token": token})


def me_status(access):
    return Browser().api("/api/v1/accounts/me", headers={"Authorization": f"Bearer {access}"})[0]


def test_four_concurrent_refreshes_with_one_token_issue_exactly_one_successor(fresh_install):
    # Arrange
    _, refresh_token = native_tokens(fresh_install)

    # Act
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        answers = list(pool.map(lambda _: refresh(refresh_token), range(4)))

    # Assert
    assert [status for status, _ in answers] == [200, 200, 200, 200], answers
    successors = {body["refresh_token"] for _, body in answers}
    assert len(successors) == 1, f"{len(successors)} different successors were issued"
    assert successors != {refresh_token}
    assert refresh(next(iter(successors)))[0] == 200, "the one successor does not refresh"


def test_a_replay_after_the_grace_revokes_the_session_within_5_seconds(fresh_install):
    # Arrange
    _, refresh_token = native_tokens(fresh_install)
    status, successor = refresh(refresh_token)
    assert status == 200, successor
    advance_clock(fresh_install, REUSE_GRACE_SECONDS + 2)

    # Act
    replay_status, _ = refresh(refresh_token)
    deadline = time.monotonic() + REVOCATION_SECONDS
    while me_status(successor["access_token"]) == 200 and time.monotonic() < deadline:
        time.sleep(0.5)
    after = me_status(successor["access_token"])
    events = fresh_install.call("db-probe", query="audit_events")

    # Assert
    assert replay_status == 401, replay_status
    assert after == 401, "the successor still works after the replay"
    assert events["refresh_reuse"] >= 1, events
    assert refresh(successor["refresh_token"])[0] == 401, "the revoked family still refreshes"


def test_a_cookie_post_without_a_proven_origin_is_403(fresh_install):
    # Arrange
    Browser().sign_in("alice", setup_token=operator_token(fresh_install, "admin", "setup-token"))
    browser = Browser()
    browser.sign_in("alice")
    route, body = "/api/v1/identity-link-attempts", {"provider": "oidc"}

    # Act
    bare = browser.api(route, "POST", body)
    foreign = browser.api(route, "POST", body, headers={"Origin": "https://evil.example"})
    cross_site = browser.api(route, "POST", body, headers={"Sec-Fetch-Site": "cross-site"})
    same_origin = browser.api(route, "POST", body, headers={"Origin": PUBLIC_ORIGIN})

    # Assert
    assert [bare[0], foreign[0], cross_site[0]] == [403, 403, 403], (bare, foreign, cross_site)
    assert same_origin[0] != 403, same_origin

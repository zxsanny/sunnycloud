"""AZ-3664 AC-2 and NFT-SEC-03 on the stack: the negative sign-in callbacks, the unverified victim email and a provider
slower than the provider timeout. Each case must end with no session, no new account and no off-site redirect, and a
valid sign-in must still work afterwards. Cases 10 and 11 of the list (a login code used twice, a wrong code verifier)
are `test_a_mobile_code_gives_session_tokens_once_and_only_to_the_verifier_holder` in `test_sign_in.py`.

The install is shared by the module (restored once); a refused step counts against the fixed sign-in limit, so each
test asks `budget` first. The stack's config sets `provider_timeout` to 2 s and the mock answers a `slow` fault after 12 s.
"""
import time

import pytest

from stackkit import Browser, advance_clock, budget, idp_fault, operator_token, restore_and_wait

PROVIDER_TIMEOUT_SECONDS = 2
OAUTH_STATE_TTL_SECONDS = 600
INTERNALS = ("panicked", "stack backtrace", "/build/", ".rs:", "sqlx", "postgres://")


@pytest.fixture(scope="module")
def install(stack_ctl):
    restore_and_wait(stack_ctl)
    admin = Browser()
    admin.sign_in("alice", setup_token=operator_token(stack_ctl, "admin", "setup-token"))
    yield stack_ctl, admin
    idp_fault(None)
    restore_and_wait(stack_ctl)


def refused(status, location):
    """A 4xx/5xx answer, or the redirect to the SPA's error route; a success redirects to the return path."""
    return status >= 400 or "error=" in location


def accounts(ctl):
    return ctl.call("db-probe", query="account_rows")["accounts"]


def assert_no_login(ctl, browser, accounts_before, bodies=()):
    assert not browser.signed_in(), "a refused callback left a session"
    assert accounts(ctl) == accounts_before, "a refused callback created an account"
    for body in bodies:
        assert not [m for m in INTERNALS if m in body], f"an answer shows internals: {body[:300]}"


def assert_valid_login_works():
    browser = Browser()
    location = browser.sign_in("alice")
    assert not refused(302, location) and browser.signed_in(), location


def prepared_callback(browser, subject="alice", **begin):
    status, started = browser.begin(**begin)
    assert status == 201, started
    return browser.consent(started["authorization_url"], subject)


def without(url, parameter):
    import urllib.parse

    parts = urllib.parse.urlparse(url)
    query = [(k, v) for k, v in urllib.parse.parse_qsl(parts.query) if k != parameter]
    return urllib.parse.urlunparse(parts._replace(query=urllib.parse.urlencode(query)))


def test_case_1_a_callback_without_state_is_refused(install):
    # Arrange
    ctl, _ = install
    before, browser = accounts(ctl), Browser()
    callback = without(prepared_callback(browser), "state")
    budget.spend(1)

    # Act
    status, location = browser.callback(callback)

    # Assert
    assert refused(status, location), (status, location)
    assert_no_login(ctl, browser, before)
    assert_valid_login_works()


def test_case_2_a_replayed_state_is_refused(install):
    # Arrange
    ctl, _ = install
    before, browser = accounts(ctl), Browser()
    callback = prepared_callback(browser)
    first_status, first_location = browser.callback(callback)
    budget.spend(1)

    # Act
    status, location = Browser().callback(callback)
    replay_status, replay_location = browser.callback(callback)

    # Assert
    assert first_status == 302 and not refused(first_status, first_location), first_location
    assert refused(status, location) and refused(replay_status, replay_location), (status, replay_status)
    assert accounts(ctl) == before


def test_case_3_a_state_issued_to_another_browser_is_refused(install):
    # Arrange
    ctl, _ = install
    before, owner, thief = accounts(ctl), Browser(), Browser()
    callback = prepared_callback(owner)
    budget.spend(1)

    # Act
    status, location = thief.callback(callback)

    # Assert
    assert refused(status, location), (status, location)
    assert_no_login(ctl, thief, before)
    assert_valid_login_works()


def test_case_4_a_state_of_one_provider_sent_to_another_providers_callback_is_refused(install):
    # Arrange
    ctl, _ = install
    before, browser = accounts(ctl), Browser()
    callback = prepared_callback(browser).replace("/providers/oidc/", "/providers/github/")
    budget.spend(1)

    # Act
    status, location = browser.callback(callback)

    # Assert
    assert callback.count("/providers/github/") == 1
    assert refused(status, location), (status, location)
    assert_no_login(ctl, browser, before)


@pytest.mark.parametrize(
    "case,fault",
    [("5 wrong nonce", "wrong-nonce"), ("6 wrong audience", "wrong-audience"), ("7 wrong issuer", "wrong-issuer")],
)
def test_cases_5_to_7_an_id_token_with_a_wrong_claim_is_refused(install, case, fault):
    # Arrange
    ctl, _ = install
    before, browser = accounts(ctl), Browser()
    callback = prepared_callback(browser)
    idp_fault(fault)
    budget.spend(1)

    # Act
    status, location = browser.callback(callback)

    # Assert
    assert refused(status, location), (case, status, location)
    assert_no_login(ctl, browser, before)
    assert_valid_login_works()


@pytest.mark.parametrize("return_to", ["https://evil.example/", "//evil.example/", "/\\evil.example"])
def test_case_8_an_absolute_or_protocol_relative_return_path_is_refused(install, return_to):
    # Arrange
    ctl, _ = install
    browser = Browser()
    budget.spend(1)

    # Act
    status, body = browser.begin(return_to=return_to)

    # Assert
    assert status != 201 and status < 500, (status, body)
    assert "authorization_url" not in str(body)


def test_case_9_a_foreign_host_header_never_reaches_a_generated_url(install):
    # Arrange
    ctl, _ = install
    browser = Browser()
    budget.spend(1)

    # Act
    status, body = browser.begin(headers={"Host": "evil.example", "X-Forwarded-Host": "evil.example"})

    # Assert
    assert status != 201 or "evil.example" not in body["authorization_url"], (status, body)


def test_an_unverified_email_of_a_victim_is_ignored(install):
    # Arrange
    ctl, _ = install
    before, attacker = accounts(ctl), Browser()
    budget.spend(1)

    # Act
    # `mallory` signs in with the email of the admin account and `email_verified: false`.
    location = attacker.sign_in("mallory")

    # Assert
    assert "invitation_required" in location, location
    assert_no_login(ctl, attacker, before)


def test_a_provider_slower_than_the_timeout_fails_the_sign_in_in_time_and_leaves_a_session_working(install):
    # Arrange
    ctl, admin = install
    before, browser = accounts(ctl), Browser()
    callback = prepared_callback(browser)
    idp_fault("slow")
    budget.spend(1)

    # Act
    started = time.monotonic()
    status, location = browser.callback(callback)
    elapsed = time.monotonic() - started
    existing = admin.api("/api/v1/accounts/me")[0]

    # Assert
    assert refused(status, location), (status, location)
    assert elapsed <= PROVIDER_TIMEOUT_SECONDS + 3, f"the provider timeout did not end the step: {elapsed:.1f} s"
    assert_no_login(ctl, browser, before)
    assert existing == 200, "an existing session stopped working"


# Last: it moves the server's test clock, which the later tests of the module would feel.
def test_case_12_a_state_older_than_its_ttl_is_refused(install):
    # Arrange
    ctl, _ = install
    before, browser = accounts(ctl), Browser()
    callback = prepared_callback(browser)
    advance_clock(ctl, OAUTH_STATE_TTL_SECONDS + 60)
    budget.spend(1)

    # Act
    status, location = browser.callback(callback)

    # Assert
    assert refused(status, location), (status, location)
    assert_no_login(ctl, browser, before)

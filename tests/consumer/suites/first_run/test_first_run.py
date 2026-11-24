"""AZ-3654 (flow F1) on the stack: a fresh install, its setup token and its first admin, through the real routes of
the api behind Caddy, with only the identity provider mocked (AC-1, AC-2, AC-4). The start refusals (AC-3) and the
printing of the token by `serve` (AC-1's source of the token) are covered by the binary tests of the server crate
(`serve_start.rs`, `serve_setup_token.rs`); here the token comes from `admin setup-token`.

Each test starts from the golden snapshot (no accounts, fixture `fresh_install` in `suites/conftest.py`).
"""
import concurrent.futures

from stackkit import Browser, operator_token


def is_refused(location):
    return "error=" in location


def me(browser):
    return browser.api("/api/v1/accounts/me")


def test_the_first_sign_in_with_the_setup_token_makes_the_first_admin(fresh_install):
    # Arrange
    browser = Browser()
    status, providers = browser.api("/api/v1/auth/providers")
    token = operator_token(fresh_install, "admin", "setup-token")

    # Act
    location = browser.sign_in("alice", setup_token=token)
    status_me, account = me(browser)
    _, providers_after = browser.api("/api/v1/auth/providers")
    reuse = Browser().sign_in("bob", setup_token=token)

    # Assert
    assert (status, providers["setup_required"]) == (200, True)
    assert not is_refused(location), location
    assert browser.signed_in()
    assert (status_me, account["role"]) == (200, "admin"), account
    assert providers_after["setup_required"] is False
    assert is_refused(reuse), f"a used setup token signed someone in: {reuse}"


def test_a_sign_in_without_the_token_creates_no_account(fresh_install):
    # Arrange
    browser = Browser()

    # Act
    location = browser.sign_in("alice")
    _, providers = browser.api("/api/v1/auth/providers")

    # Assert
    assert "invitation_required" in location, location
    assert not browser.signed_in()
    assert providers["setup_required"] is True


def test_two_concurrent_sign_ins_with_the_same_token_make_exactly_one_admin(fresh_install):
    # Arrange
    token = operator_token(fresh_install, "admin", "setup-token")
    subjects = ["alice", "bob"]
    browsers = {name: Browser() for name in subjects}

    # Act
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        futures = {name: pool.submit(browsers[name].sign_in, name, token) for name in subjects}
        locations = {name: future.result() for name, future in futures.items()}

    # Assert
    signed_in = [name for name in subjects if browsers[name].signed_in() and not is_refused(locations[name])]
    assert len(signed_in) == 1, locations
    assert [is_refused(locations[name]) for name in subjects if name not in signed_in] == [True]


def test_a_new_setup_token_replaces_the_lost_one(fresh_install):
    # Arrange
    old = operator_token(fresh_install, "admin", "setup-token")
    new = operator_token(fresh_install, "admin", "setup-token")

    # Act
    with_old = Browser().sign_in("alice", setup_token=old)
    browser = Browser()
    with_new = browser.sign_in("alice", setup_token=new)

    # Assert
    assert old != new
    assert is_refused(with_old), with_old
    assert not is_refused(with_new) and browser.signed_in(), with_new


def test_an_admin_invite_makes_a_second_admin(fresh_install):
    # Arrange
    first = Browser()
    first.sign_in("alice", setup_token=operator_token(fresh_install, "admin", "setup-token"))
    invite = operator_token(fresh_install, "admin", "create")

    # Act
    second = Browser()
    location = second.sign_in("bob", invite_token=invite)
    status, account = me(second)

    # Assert
    assert not is_refused(location), location
    assert (status, account["role"]) == (200, "admin"), account


def test_an_admin_invite_is_single_use(fresh_install):
    # Arrange
    Browser().sign_in("alice", setup_token=operator_token(fresh_install, "admin", "setup-token"))
    invite = operator_token(fresh_install, "admin", "create")
    Browser().sign_in("bob", invite_token=invite)

    # Act
    # A third identity: an existing account signs in again without any invite, so it would prove nothing.
    again = Browser().sign_in("dev-seed-user-1", invite_token=invite)

    # Assert
    assert is_refused(again), f"a used invite signed a third person in: {again}"

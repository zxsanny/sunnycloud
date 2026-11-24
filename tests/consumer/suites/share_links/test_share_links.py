"""AZ-3662 (flow F11) on the stack: share links through the real routes, the real storage and the real download path.
The owner is a browser session; the visitor has no credentials at all. The stack's config sets the share link life
to 30 s (60 s at most) and the wrong-password window to 10 s, so the test clock moves by seconds, not minutes.

Every test starts from the golden snapshot (an install with no accounts and no files).
"""
import json
import time

from stackkit import Browser, Drive, advance_clock, device_token, operator_token, refusal_shape

BASE = "https://sunnycloud.test"
CONTENT = "hello share"
WRONG_PASSWORD_LIMIT = 10
WRONG_PASSWORD_WINDOW_SECONDS = 10
SHARE_TTL_SECONDS = 5
REVOCATION_SECONDS = 5
SECRET_KEYS = {"token", "code", "secret", "password", "email", "name", "refresh", "access", "invite", "setup_token"}


def owner_with_file(ctl, name="hello.txt", data=CONTENT.encode()):
    owner = Browser()
    owner.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
    drive = Drive(owner)
    root = drive.root()
    return owner, drive, root, drive.upload(root, name, data)


def create_share(drive, node_id, **extra):
    status, share = drive.call("/api/v1/shares", "POST", {"node_id": node_id, **extra})
    assert status == 201, share
    return share


def open_link(token):
    """A visitor with no credentials asks for the link's file: `(status, headers, text)` of the content."""
    return Browser().request(f"{BASE}/api/v1/public-shares/{token}/content")


def uniform(token):
    visitor = Browser()
    answers = [visitor.request(f"{BASE}/api/v1/public-shares/{token}"), visitor.request(f"{BASE}/api/v1/public-shares/{token}/content")]
    return [refusal_shape(*answer) for answer in answers]


def test_a_link_opens_without_an_account_serves_the_bytes_and_stores_only_a_hash(fresh_install):
    # Arrange
    _, drive, _, file = owner_with_file(fresh_install)

    # Act
    share = create_share(drive, file["node_id"])
    listing = drive.ok(200, "/api/v1/shares")
    status, meta = Browser().api(f"/api/v1/public-shares/{share['token']}")
    content_status, headers, text = open_link(share["token"])
    columns = fresh_install.call("db-probe", query="share_link_columns")["values"]
    events = fresh_install.call("db-probe", query="audit_events")
    keys = fresh_install.call("db-probe", query="audit_detail_keys")["values"]

    # Assert
    assert share["token"] in share["url_path"], share
    assert share["token"] not in json.dumps(listing), "the list of links shows a token"
    assert (status, meta["name"], meta["size"]) == (200, "hello.txt", len(CONTENT)), meta
    assert (content_status, text) == (200, CONTENT)
    assert headers.get("X-Content-Type-Options", "").lower() == "nosniff", headers
    assert headers.get("Content-Disposition", "").startswith("attachment"), "a text file is not on the inline allowlist"
    assert "token_hash" in columns and "token" not in columns, columns
    assert events["share_created"] >= 1 and events["share_accessed"] >= 1, events
    assert not SECRET_KEYS & set(keys), f"the audit detail names a secret or a person: {keys}"


def test_a_password_link_asks_for_it_stops_the_eleventh_wrong_attempt_and_opens_after_the_window(fresh_install):
    # Arrange
    _, drive, _, file = owner_with_file(fresh_install)
    share = create_share(drive, file["node_id"], password="correct horse", expires_in=60)
    token = share["token"]
    visitor = Browser()

    # Act
    first = visitor.api(f"/api/v1/public-shares/{token}")
    wrong = [visitor.api(f"/api/v1/public-shares/{token}/password-verifications", "POST", {"password": "nope"})[0]
             for _ in range(WRONG_PASSWORD_LIMIT)]
    eleventh = visitor.request(f"{BASE}/api/v1/public-shares/{token}/password-verifications", "POST", {"password": "nope"})
    still_blocked = visitor.api(f"/api/v1/public-shares/{token}/password-verifications", "POST", {"password": "correct horse"})[0]
    # The limiter is a token bucket on the monotonic clock, which the test clock does not move: wait for the
    # configured window itself (10 s here) and take the right password as soon as the limiter lets it through.
    deadline = time.monotonic() + WRONG_PASSWORD_WINDOW_SECONDS + 3
    while True:
        right = visitor.api(f"/api/v1/public-shares/{token}/password-verifications", "POST", {"password": "correct horse"})[0]
        if right != 429 or time.monotonic() > deadline:
            break
        time.sleep(1)
    content = visitor.request(f"{BASE}/api/v1/public-shares/{token}/content")

    # Assert
    assert (first[0], first[1]["code"]) == (401, "password_required"), first
    assert wrong == [401] * WRONG_PASSWORD_LIMIT, wrong
    assert eleventh[0] == 429 and int(eleventh[1].get("Retry-After", "0")) >= 1, eleventh[:2]
    assert still_blocked == 429, "the right password was accepted inside the window"
    assert right == 204, right
    assert (content[0], content[2]) == (200, CONTENT), content[:1]


def test_an_expired_a_revoked_and_a_random_link_answer_alike_and_a_revocation_is_quick(fresh_install):
    # Arrange
    _, drive, _, file = owner_with_file(fresh_install)
    valid = create_share(drive, file["node_id"])
    short = create_share(drive, file["node_id"], expires_in=SHARE_TTL_SECONDS)
    revoked = create_share(drive, file["node_id"])
    assert open_link(valid["token"])[0] == 200 and open_link(short["token"])[0] == 200

    # Act
    drive.ok(204, f"/api/v1/shares/{revoked['id']}", "DELETE")
    deadline = time.monotonic() + REVOCATION_SECONDS
    while open_link(revoked["token"])[0] == 200 and time.monotonic() < deadline:
        time.sleep(0.25)
    advance_clock(fresh_install, SHARE_TTL_SECONDS + 1)
    shapes = {"expired": uniform(short["token"]), "revoked": uniform(revoked["token"]), "random": uniform("not-a-token-" + "x" * 40)}
    still_valid = open_link(valid["token"])[0]

    # Assert
    assert shapes["expired"][0][0] == 404, shapes
    assert shapes["expired"] == shapes["revoked"] == shapes["random"], shapes
    assert still_valid == 200


def test_a_device_token_a_folder_and_a_missing_origin_are_refused_when_creating_a_link(fresh_install):
    # Arrange
    owner, drive, root, file = owner_with_file(fresh_install)
    folder = drive.folder(root, "docs")
    desktop = device_token(owner)

    # Act
    by_device = Browser().api("/api/v1/shares", "POST", {"node_id": file["node_id"]}, {"Authorization": f"Bearer {desktop}"})
    for_folder = drive.call("/api/v1/shares", "POST", {"node_id": folder})
    no_origin = owner.api("/api/v1/shares", "POST", {"node_id": file["node_id"]})

    # Assert
    assert by_device[0] == 403, by_device
    assert 400 <= for_folder[0] < 500 and for_folder[0] != 403 and "code" in for_folder[1], for_folder
    assert no_origin[0] == 403, no_origin


def test_a_link_follows_its_file_through_rename_move_and_edit_and_dies_with_it(fresh_install):
    # Arrange
    _, drive, root, file = owner_with_file(fresh_install)
    token = create_share(drive, file["node_id"], expires_in=60)["token"]
    box = drive.folder(root, "box")

    def serves(expected, within=3.0):
        deadline = time.monotonic() + within
        while True:
            status, _, text = open_link(token)
            if (status, text) == expected or time.monotonic() >= deadline:
                return status, text
            time.sleep(0.25)

    # Act
    drive.rename(file["node_id"], "renamed.txt")
    after_rename = serves((200, CONTENT))
    drive.move(file["node_id"], box)
    after_move = serves((200, CONTENT))
    edited = drive.upload(box, "renamed.txt", b"edited content", base_revision=file["revision_id"])
    after_edit = serves((200, "edited content"))
    drive.delete(file["node_id"])
    gone = serves((404, None), within=REVOCATION_SECONDS)

    # Assert
    assert edited["node_id"] == file["node_id"], "an edit with a base revision made another file"
    assert after_rename == (200, CONTENT) and after_move == (200, CONTENT), (after_rename, after_move)
    assert after_edit == (200, "edited content"), after_edit
    assert gone[0] == 404, f"the link still answered {gone[0]} after the file was deleted"

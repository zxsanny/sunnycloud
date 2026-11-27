"""ST-09-2 (AC-25) on H2: the worker container has no path to the database or the blobs. A compromised supervisor finds
no database variable, no secret but its own token, no blob volume, no Docker socket and no capability; the database is
reachable on the network by design, and a login from the worker's own network position is refused for every role.

The login is tried by e2e-ctl from a throwaway container that shares the worker's network namespace (the worker image
has no client): a startup message for each role with an empty password, which libpq never sends, so a server that asks
for a password refuses it. IT-09-1 checks the environment, secrets and blob mount at the start-up gate as well."""


def test_st_09_2_the_worker_container_holds_no_credential_mount_socket_or_capability_beyond_its_token(stack_ctl):
    # Act
    facts = stack_ctl.container_facts()

    # Assert
    database = [name for name in facts["env_names"]
                if "DB" in name.upper().split("_") or name.upper().startswith(("DATABASE", "PG", "POSTGRES"))]
    assert database == [], facts["env_names"]
    assert facts["secrets"] == ["worker_api_token"]
    assert facts["mounts"] == ["/run/decoder-general", "/run/decoder-interactive", "/run/secrets/worker_api_token",
                               "/scratch-general", "/scratch-interactive"]
    assert [name for name in facts["volumes"] if name in ("blobs", "pgdata")] == [], facts["volumes"]
    assert facts["docker_socket"] is False
    assert (facts["cap_add"], facts["cap_drop"], facts["privileged"]) == ([], ["ALL"], False)
    assert facts["networks"] == ["backend"]


def test_st_09_2_a_login_from_the_workers_network_position_reaches_the_database_and_is_refused_for_every_role(stack_ctl):
    # Act
    roles = stack_ctl.call("inspect", db_login="worker")["roles"]

    # Assert
    assert {role: answer["result"] for role, answer in roles.items()} == {role: "refused" for role in roles}, roles
    assert set(roles) == {"sunnycloud", "sunnycloud_migrator", "postgres", "no_such_role"}

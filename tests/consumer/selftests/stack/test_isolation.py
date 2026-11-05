import errno
import socket

DB_PORT = 5432


def test_consumer_cannot_resolve_the_database_name():
    # Arrange
    name = "db"

    # Act
    try:
        socket.getaddrinfo(name, DB_PORT)
        resolved = True
    except socket.gaierror:
        resolved = False

    # Assert
    assert not resolved, "the consumer resolved 'db': it shares a network with the database"


def test_consumer_cannot_open_a_tcp_connection_to_the_database_address(db_addr):
    # Arrange
    address = db_addr

    # Act
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.settimeout(3)
        outcome = probe.connect_ex((address, DB_PORT))

    # Assert
    assert outcome != 0, f"TCP connection to {address}:{DB_PORT} succeeded: the consumer has a route to backend"
    assert outcome in (errno.EHOSTUNREACH, errno.ENETUNREACH, errno.ETIMEDOUT, errno.ECONNREFUSED, errno.EAGAIN), \
        f"unexpected connect error {errno.errorcode.get(outcome, outcome)}"

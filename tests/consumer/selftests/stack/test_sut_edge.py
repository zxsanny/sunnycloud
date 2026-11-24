"""The stack's front door: what the consumer, a client on client-net, can and cannot reach (environment.md, Networks)."""
import json
import socket
import ssl
import urllib.request

from conftest import required_env

SERVER_INFO = "/api/v1/server-info"
DISCOVERY = "/.well-known/openid-configuration"
BACKEND_NAMES = ["api", "worker", "decoder-interactive", "decoder-general", "migrate", "keycheck"]


def fetch_json(url):
    with urllib.request.urlopen(url, timeout=10) as answer:
        return answer.status, json.load(answer)


def test_caddy_serves_server_info_over_a_certificate_the_exported_root_signs():
    # Arrange
    url = required_env("E2E_BASE_URL") + SERVER_INFO

    # Act
    status, body = fetch_json(url)

    # Assert
    assert status == 200 and isinstance(body, dict) and body, f"server-info answered {status}: {body!r}"


def test_the_public_listener_negotiates_tls_1_3():
    # Arrange
    context = ssl.create_default_context()
    context.minimum_version = ssl.TLSVersion.TLSv1_3

    # Act
    with socket.create_connection(("sunnycloud.test", 443), timeout=10) as raw:
        with context.wrap_socket(raw, server_hostname="sunnycloud.test") as tls:
            version = tls.version()

    # Assert
    assert version == "TLSv1.3"


def test_the_mock_identity_provider_names_the_issuer_the_server_is_configured_with():
    # Arrange
    base = required_env("E2E_IDP_URL")

    # Act
    status, document = fetch_json(base + DISCOVERY)

    # Assert
    assert (status, document.get("issuer")) == (200, base)


def test_no_backend_service_name_resolves_from_the_consumer():
    # Arrange and Act
    resolved = []
    for name in BACKEND_NAMES:
        try:
            socket.getaddrinfo(name, 8080)
            resolved.append(name)
        except socket.gaierror:
            pass

    # Assert
    assert not resolved, f"the consumer resolves backend names {resolved}: it shares a network with them"

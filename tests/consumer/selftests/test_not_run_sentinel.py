import pytest


@pytest.mark.not_run("NO-SERVICE", "the api service is not part of the core profile yet")
def test_api_health_needs_the_api_service():
    raise AssertionError("must not run: the not_run marker has to skip this test")

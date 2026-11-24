import pytest


@pytest.mark.not_run("NO-SERVICE", "synthetic: proves that a NOT-RUN test is reported as NOT-RUN and never as a pass")
def test_a_not_run_test_is_never_a_pass():
    raise AssertionError("must not run: the not_run marker has to skip this test")

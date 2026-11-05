import os

pytest_plugins = ["pytester"]

# The seeded failure proves that a FAIL fails the run (AC-3); it is not collected unless asked for.
collect_ignore = [] if os.environ.get("SEED_FAIL") == "1" else ["selftests/test_seeded_failure.py"]

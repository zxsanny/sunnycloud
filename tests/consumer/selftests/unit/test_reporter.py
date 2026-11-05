import csv
import json
from pathlib import Path

import pytest

import e2e_report

EXPECTED_COLUMNS = ["Test ID", "Test Name", "Execution Time (ms)", "Result", "Error Message"]


@pytest.fixture
def run_inner(pytester, monkeypatch, tmp_path):
    results = tmp_path / "results"
    monkeypatch.setenv("E2E_RESULTS_DIR", str(results))
    monkeypatch.delenv("SEED_FAIL", raising=False)
    monkeypatch.setenv("PYTHONPATH", str(Path(e2e_report.__file__).parent))

    def run(source):
        pytester.makepyfile(test_inner=source)
        result = pytester.runpytest_subprocess("-v", "--tb=short", "-p", "e2e_report", "-p", "no:cacheprovider")
        return result, results

    return run


def read_rows(results):
    with (results / "report.csv").open(newline="", encoding="utf-8") as report:
        return list(csv.reader(report))


def test_reporter_pass_fail_skip_writes_one_row_each_with_the_five_columns(run_inner):
    # Arrange
    source = """
import pytest
def test_ok(): pass
def test_bad(): assert 1 == 2, "one is not two"
@pytest.mark.skip(reason="plain skip")
def test_skipped(): pass
"""

    # Act
    result, results = run_inner(source)
    rows = read_rows(results)

    # Assert
    assert rows[0] == EXPECTED_COLUMNS
    by_name = {row[1]: row for row in rows[1:]}
    assert {name: row[3] for name, row in by_name.items()} == {"test_ok": "PASS", "test_bad": "FAIL", "test_skipped": "SKIP"}
    assert "one is not two" in by_name["test_bad"][4]
    assert by_name["test_skipped"][4] == "plain skip"
    assert all(row[2].isdigit() and len(row) == 5 for row in rows[1:])
    assert result.ret == 1


def test_reporter_all_pass_exits_zero_with_summary_and_metrics(run_inner):
    # Arrange
    source = "def test_a(): pass\ndef test_b(): pass\n"

    # Act
    result, results = run_inner(source)
    metrics = json.loads((results / "metrics.json").read_text(encoding="utf-8"))

    # Assert
    assert result.ret == 0
    result.stdout.fnmatch_lines(["PASS=2 FAIL=0 NOT-RUN=0 *"])
    assert (metrics["pass"], metrics["fail"], metrics["total"], metrics["not_run"]) == (2, 0, 2, {})
    assert metrics["reference_hardware_ids"] == []


def test_reporter_setup_error_is_a_fail_row(run_inner):
    # Arrange
    source = """
import pytest
@pytest.fixture
def broken(): raise RuntimeError("fixture exploded")
def test_uses_broken(broken): pass
"""

    # Act
    result, results = run_inner(source)
    rows = read_rows(results)

    # Assert
    assert [row[3] for row in rows[1:]] == ["FAIL"]
    assert "fixture exploded" in rows[1][4]
    assert result.ret != 0


def test_reporter_collection_error_is_a_fail_row(run_inner):
    # Arrange
    source = "import module_that_does_not_exist\ndef test_never(): pass\n"

    # Act
    result, results = run_inner(source)
    rows = read_rows(results)

    # Assert
    assert [row[3] for row in rows[1:]] == ["FAIL"]
    assert "module_that_does_not_exist" in rows[1][4]
    assert result.ret != 0


def test_not_run_marker_gives_prefixed_skip_not_run_csv_entry_and_summary_count(run_inner):
    # Arrange
    source = """
import pytest
@pytest.mark.not_run("NO-SERVICE", "api is not started")
def test_a(): raise AssertionError("must be skipped")
@pytest.mark.not_run(code="NO-SERVICE", reason="api is not started")
def test_b(): raise AssertionError("must be skipped")
@pytest.mark.not_run("HW-REF", "no reference hardware")
def test_c(): raise AssertionError("must be skipped")
"""

    # Act
    result, results = run_inner(source)
    rows = read_rows(results)
    with (results / "not-run.csv").open(newline="", encoding="utf-8") as not_run:
        not_run_rows = list(csv.reader(not_run))

    # Assert
    assert {(row[3], row[4].split(":")[0]) for row in rows[1:]} == {("SKIP", "NOT-RUN(NO-SERVICE)"), ("SKIP", "NOT-RUN(HW-REF)")}
    assert not_run_rows[0] == ["Test ID", "Code", "Reason"]
    assert sorted(row[1] for row in not_run_rows[1:]) == ["HW-REF", "NO-SERVICE", "NO-SERVICE"]
    assert result.ret == 0
    result.stdout.fnmatch_lines(["PASS=0 FAIL=0 NOT-RUN=3 *", "NOT-RUN(HW-REF): 1", "NOT-RUN(NO-SERVICE): 2"])


def test_skip_not_run_helper_inside_a_test_is_a_not_run_row(run_inner):
    # Arrange
    source = """
from e2e_report import skip_not_run
def test_needs_device(): skip_not_run("H-ANDROID", "no device attached")
"""

    # Act
    result, results = run_inner(source)
    rows = read_rows(results)

    # Assert
    assert rows[1][3:] == ["SKIP", "NOT-RUN(H-ANDROID): no device attached"]
    assert result.ret == 0


def test_plain_skip_is_not_counted_as_not_run(run_inner):
    # Arrange
    source = "import pytest\n@pytest.mark.skip(reason='x')\ndef test_a(): pass\n"

    # Act
    result, results = run_inner(source)
    metrics = json.loads((results / "metrics.json").read_text(encoding="utf-8"))

    # Assert
    assert (metrics["skip"], metrics["not_run"], metrics["skip_without_not_run_code"]) == (1, {}, 1)
    assert not (results / "not-run.csv").read_text(encoding="utf-8").splitlines()[1:]


@pytest.mark.parametrize("marker, complaint", [
    ('not_run("NO-SERVICE")', "exactly (code, reason)"),
    ('not_run("bad code!", "reason")', "code must match"),
    ('not_run("NO-SERVICE", " ")', "non-empty reason"),
])
def test_not_run_marker_misuse_is_a_usage_error(run_inner, marker, complaint):
    # Arrange
    source = f"import pytest\n@pytest.mark.{marker}\ndef test_a(): pass\n"

    # Act
    result, _ = run_inner(source)

    # Assert
    assert result.ret == 4
    assert complaint in result.stderr.str()

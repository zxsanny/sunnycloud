"""pytest plugin of the black-box harness: CSV report, NOT-RUN rule and end-of-run summary.

Load with `pytest -v --tb=short -p e2e_report`. Output directory: E2E_RESULTS_DIR (default /results).

    report.csv   Test ID, Test Name, Execution Time (ms), Result (PASS/FAIL/SKIP), Error Message
    not-run.csv  Test ID, Code, Reason   (every SKIP whose message starts with `NOT-RUN(<code>): `)
    metrics.json counts, duration, versions, reference-hardware ids

A test that cannot run is marked `@pytest.mark.not_run("<code>", "<reason>")` or calls
`skip_not_run(code, reason)`; it is reported SKIP, never PASS, and counted per code in the summary.
Any FAIL (or error) makes pytest exit non-zero.
"""
import csv
import json
import os
import re
import sys
import time
from collections import Counter
from pathlib import Path

import pytest

COLUMNS = ["Test ID", "Test Name", "Execution Time (ms)", "Result", "Error Message"]
NOT_RUN_COLUMNS = ["Test ID", "Code", "Reason"]
NOT_RUN_PATTERN = re.compile(r"^NOT-RUN\(([A-Za-z0-9][A-Za-z0-9_-]*)\): (.+)$", re.DOTALL)
CODE_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]*$")
MAX_MESSAGE_CHARS = 500


def skip_not_run(code, reason):
    """Skip the running test as NOT-RUN(<code>)."""
    _check_marker_args(code, reason, "skip_not_run")
    pytest.skip(f"NOT-RUN({code}): {reason}")


def _check_marker_args(code, reason, where):
    if not isinstance(code, str) or not CODE_PATTERN.match(code):
        raise pytest.UsageError(f"{where}: code must match {CODE_PATTERN.pattern}, got {code!r}")
    if not isinstance(reason, str) or not reason.strip():
        raise pytest.UsageError(f"{where}: a non-empty reason is required for code {code}")


def _one_line(text):
    collapsed = " | ".join(part.strip() for part in str(text).splitlines() if part.strip())
    return collapsed if len(collapsed) <= MAX_MESSAGE_CHARS else collapsed[: MAX_MESSAGE_CHARS - 3] + "..."


def _failure_message(report):
    crash = getattr(report.longrepr, "reprcrash", None)
    message = getattr(crash, "message", None) or str(report.longrepr)
    return _one_line(message) or "failed without a message"


def _skip_message(report):
    longrepr = report.longrepr
    message = longrepr[2] if isinstance(longrepr, tuple) else str(longrepr)
    return _one_line(message.removeprefix("Skipped: "))


class Reporter:
    def __init__(self, results_dir):
        self.results_dir = Path(results_dir)
        self.results_dir.mkdir(parents=True, exist_ok=True)
        self.started = time.time()
        self.counts = Counter()
        self.not_run = Counter()
        self.not_run_rows = []
        self._pending = {}
        self._report_file = (self.results_dir / "report.csv").open("w", newline="", encoding="utf-8")
        self._writer = csv.writer(self._report_file)
        self._writer.writerow(COLUMNS)
        self._report_file.flush()

    def _record(self, test_id, name, duration_ms, result, message):
        self._writer.writerow([test_id, name, duration_ms, result, message])
        self._report_file.flush()
        self.counts[result] += 1
        match = NOT_RUN_PATTERN.match(message) if result == "SKIP" else None
        if match:
            self.not_run[match.group(1)] += 1
            self.not_run_rows.append([test_id, match.group(1), match.group(2)])

    def pytest_runtest_logreport(self, report):
        entry = self._pending.setdefault(report.nodeid, {"duration": 0.0, "failure": None, "skip": None})
        entry["duration"] += report.duration
        if report.failed and entry["failure"] is None:
            entry["failure"] = f"{report.when}: {_failure_message(report)}" if report.when != "call" else _failure_message(report)
        elif report.skipped and entry["skip"] is None:
            entry["skip"] = _skip_message(report)
        if report.when == "teardown":
            self._finish(report.nodeid, self._pending.pop(report.nodeid))

    def _finish(self, nodeid, entry):
        name = nodeid.rsplit("::", 1)[-1]
        duration_ms = round(entry["duration"] * 1000)
        if entry["failure"] is not None:
            self._record(nodeid, name, duration_ms, "FAIL", entry["failure"])
        elif entry["skip"] is not None:
            self._record(nodeid, name, duration_ms, "SKIP", entry["skip"])
        else:
            self._record(nodeid, name, duration_ms, "PASS", "")

    def pytest_collectreport(self, report):
        if not report.failed:
            return
        self._record(report.nodeid or "collection", report.nodeid or "collection", 0, "FAIL",
                     f"collection: {_failure_message(report)}")

    @property
    def uncoded_skips(self):
        return self.counts["SKIP"] - sum(self.not_run.values())

    def pytest_terminal_summary(self, terminalreporter):
        write = terminalreporter.write_line
        terminalreporter.section("e2e summary")
        write(f"PASS={self.counts['PASS']} FAIL={self.counts['FAIL']} NOT-RUN={sum(self.not_run.values())}"
              f" (other SKIP without a NOT-RUN code: {self.uncoded_skips})")
        for code, count in sorted(self.not_run.items()):
            write(f"NOT-RUN({code}): {count}")
        write(f"report: {self.results_dir / 'report.csv'}")

    def pytest_sessionfinish(self, exitstatus):
        self._report_file.close()
        with (self.results_dir / "not-run.csv").open("w", newline="", encoding="utf-8") as out:
            writer = csv.writer(out)
            writer.writerow(NOT_RUN_COLUMNS)
            writer.writerows(self.not_run_rows)
        metrics = {
            "profile": os.environ.get("E2E_PROFILE", ""),
            "total": sum(self.counts.values()),
            "pass": self.counts["PASS"],
            "fail": self.counts["FAIL"],
            "skip": self.counts["SKIP"],
            "not_run": dict(sorted(self.not_run.items())),
            "skip_without_not_run_code": self.uncoded_skips,
            "duration_ms": round((time.time() - self.started) * 1000),
            "pytest_exit_status": int(exitstatus),
            "python": sys.version.split()[0],
            "pytest": pytest.__version__,
            "reference_hardware_ids": [],
        }
        (self.results_dir / "metrics.json").write_text(json.dumps(metrics, indent=2) + "\n", encoding="utf-8")


def _marker_args(marker):
    code = marker.args[0] if marker.args else marker.kwargs.get("code")
    reason = marker.args[1] if len(marker.args) > 1 else marker.kwargs.get("reason")
    return code, reason


def pytest_configure(config):
    config.addinivalue_line("markers", "not_run(code, reason): report the test as SKIP `NOT-RUN(<code>): <reason>`, never PASS")
    config.pluginmanager.register(Reporter(os.environ.get("E2E_RESULTS_DIR", "/results")), "e2e-reporter")


def pytest_collection_modifyitems(items):
    for item in items:
        for marker in item.iter_markers("not_run"):
            if len(marker.args) + len(marker.kwargs) != 2:
                raise pytest.UsageError(f"{item.nodeid}: not_run needs exactly (code, reason)")
            _check_marker_args(*_marker_args(marker), item.nodeid)


@pytest.hookimpl(tryfirst=True)
def pytest_runtest_setup(item):
    marker = item.get_closest_marker("not_run")
    if marker is not None:
        code, reason = _marker_args(marker)
        pytest.skip(f"NOT-RUN({code}): {reason}")

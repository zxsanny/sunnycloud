"""Runs the IT cases of a component test specification and checks that each has tests and keeps its budget.

usage: it_suite.py SPEC_MD CASES_FILE PACKAGE WORKSPACE_DIR
SPEC_MD: the component's tests.md; each `### IT-<c>-<n>:` heading is a case and its `**Max execution time**: N s`
line the budget.
CASES_FILE: per line `<case id> <test target> <prefix>...`; `#` starts a comment; a case may take several lines.
A prefix selects every test of the target whose full name starts with it.
Each target is built and listed first (`cargo test --locked -p PACKAGE --test TARGET -- --list`, not timed); each
case then runs its selected tests with `--exact` and is reported with its result, test count and duration.
It fails when a case has no line, a line names a case the spec lacks, a prefix selects no test, a run fails or
passes no test, or a case takes longer than its budget.
CARGO (environment) overrides the cargo command, for the tests of this script.
Exit 0 pass, 1 failure, 2 usage.
"""

import os
import re
import subprocess
import sys
import time

CASE_HEADING = re.compile(r"^### (IT-\d+-\d+):")
BUDGET_LINE = re.compile(r"^\*\*Max execution time\*\*:\s*(\d+(?:\.\d+)?)\s*s\b")
RESULT_LINE = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed")
TAG = "[it-suite]"


def read_spec(path: str) -> dict[str, float | None]:
    """Case ids in spec order, each with its budget in seconds (None when the case states none)."""
    budgets: dict[str, float | None] = {}
    current = None
    with open(path) as handle:
        for line in handle:
            heading = CASE_HEADING.match(line)
            if heading:
                current = heading.group(1)
                budgets[current] = None
            elif line.startswith("#"):
                current = None
            elif current:
                budget = BUDGET_LINE.match(line)
                if budget:
                    budgets[current] = float(budget.group(1))
    return budgets


def read_cases(path: str) -> dict[str, list[tuple[str, list[str]]]]:
    """Case id -> list of (target, prefixes), in file order."""
    cases: dict[str, list[tuple[str, list[str]]]] = {}
    with open(path) as handle:
        for number, raw in enumerate(handle, 1):
            words = raw.split("#", 1)[0].split()
            if not words:
                continue
            if len(words) < 3:
                raise ValueError(f"{path}:{number}: needs a case id, a test target and at least one prefix")
            cases.setdefault(words[0], []).append((words[1], words[2:]))
    return cases


def cargo(package: str, target: str, test_args: list[str], workspace: str, echo: bool) -> tuple[int, list[str]]:
    """Exit code and stdout lines of one cargo test call. stderr (build progress) passes through and stdout is
    echoed as it comes when `echo` is set, so the watchdog never sees a silent build or run."""
    command = [os.environ.get("CARGO", "cargo"), "test", "--locked", "-p", package, "--test", target, "--", *test_args]
    lines = []
    with subprocess.Popen(command, cwd=workspace, stdout=subprocess.PIPE, text=True) as process:
        assert process.stdout is not None
        for line in process.stdout:
            lines.append(line.rstrip("\n"))
            if echo:
                sys.stdout.write(line)
                sys.stdout.flush()
    return process.returncode, lines


def list_tests(package: str, target: str, workspace: str) -> list[str]:
    code, lines = cargo(package, target, ["--list"], workspace, echo=False)
    if code != 0:
        print("\n".join(lines))
        raise RuntimeError(f"listing the tests of target {target} failed with exit {code}")
    return [line[: -len(": test")] for line in lines if line.endswith(": test")]


def run_case(
    case: str,
    lines: list[tuple[str, list[str]]],
    listed: dict[str, list[str]],
    package: str,
    workspace: str,
) -> tuple[int, list[str], float]:
    """(passed tests, failures, seconds) of one case; the time covers only its test runs."""
    failures: list[str] = []
    passed = 0
    seconds = 0.0
    for target, prefixes in lines:
        selected: list[str] = []
        for prefix in prefixes:
            matched = [name for name in listed[target] if name.startswith(prefix)]
            if not matched:
                failures.append(f"{case}: prefix {prefix} selects no test of target {target}")
            selected.extend(name for name in matched if name not in selected)
        if not selected:
            continue
        start = time.monotonic()
        code, output = cargo(package, target, ["--exact", *selected], workspace, echo=True)
        seconds += time.monotonic() - start
        matches = (RESULT_LINE.match(line) for line in output)
        run_passed = sum(int(match.group(1)) for match in matches if match)
        passed += run_passed
        if code != 0:
            failures.append(f"{case}: target {target} failed with exit {code}")
        elif run_passed == 0:
            failures.append(f"{case}: target {target} passed no test")
    return passed, failures, seconds


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    spec_path, cases_path, package, workspace = sys.argv[1:]
    budgets = read_spec(spec_path)
    try:
        cases = read_cases(cases_path)
    except ValueError as error:
        print(f"{TAG} FAILED: {error}", file=sys.stderr)
        return 2
    print(f"{TAG} {len(budgets)} cases in {spec_path}; tests from {cases_path}")
    failures = [f"{case} is not a case of {spec_path}" for case in cases if case not in budgets]

    targets = sorted({target for lines in cases.values() for target, _ in lines})
    listed = {}
    for target in targets:
        print(f"{TAG} build and list target {target} (not timed)", flush=True)
        try:
            listed[target] = list_tests(package, target, workspace)
        except RuntimeError as error:
            print(f"{TAG} FAILED: {error}", file=sys.stderr)
            return 1

    report = []
    for case, budget in budgets.items():
        if case not in cases:
            failures.append(f"{case} has no test")
            report.append((case, "MISSING", 0, 0.0, budget))
            continue
        print(f"{TAG} === {case}", flush=True)
        passed, case_failures, seconds = run_case(case, cases[case], listed, package, workspace)
        if budget is None:
            case_failures.append(f"{case} states no max execution time")
        elif seconds > budget:
            case_failures.append(f"{case} took {seconds:.2f}s, over its max execution time of {budget:g}s")
        failures.extend(case_failures)
        report.append((case, "FAIL" if case_failures else "PASS", passed, seconds, budget))

    print(f"{TAG} case      result   tests  duration  budget")
    for case, verdict, passed, seconds, budget in report:
        shown = "none" if budget is None else f"{budget:g}s"
        print(f"{TAG} {case:<9} {verdict:<8} {passed:>5}  {seconds:>7.2f}s  {shown}")
    for failure in failures:
        print(f"{TAG} FAILED: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(f"{TAG} OK: {len(report)} cases passed within their budgets")
    return 0


if __name__ == "__main__":
    sys.exit(main())

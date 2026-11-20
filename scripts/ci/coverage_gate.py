"""Applies the coverage thresholds to a `cargo llvm-cov report --json --summary-only` file (or the merge_lcov.py output).

usage: coverage_gate.py SUMMARY_JSON WORKSPACE_DIR CRITICAL_PATHS_FILE MIN_OVERALL MIN_CRITICAL
Line coverage is covered/total instrumented lines; crates and files without instrumented lines do not count.
CRITICAL_PATHS_FILE: one path per line, relative to WORKSPACE_DIR (a file or a directory prefix), optionally followed
by its own minimum percentage (a per-crate floor such as `components/x/src 75`); `#` starts a comment.
Every entry is measured on its own and must reach its minimum, MIN_CRITICAL when none is given; an entry that
matches no file fails.
Exit 0 pass, 1 below a threshold or unmeasurable, 2 usage.
"""

import json
import sys
from pathlib import Path


def percent(covered, count):
    return 100.0 * covered / count


def main():
    if len(sys.argv) != 6:
        print(__doc__, file=sys.stderr)
        return 2
    summary_path, workspace, critical_path, min_overall, min_critical = sys.argv[1:]
    min_overall, min_critical = float(min_overall), float(min_critical)
    workspace = Path(workspace).resolve()

    with open(summary_path) as handle:
        files = json.load(handle)["data"][0]["files"]
    measured = {}
    for entry in files:
        lines = entry["summary"]["lines"]
        try:
            relative = str(Path(entry["filename"]).resolve().relative_to(workspace))
        except ValueError:
            continue
        measured[relative] = (lines["covered"], lines["count"])

    failures = []
    total_covered = sum(covered for covered, _ in measured.values())
    total_count = sum(count for _, count in measured.values())
    print(f"[coverage] {len(measured)} files with instrumented lines, {total_count} lines")
    if total_count == 0:
        failures.append("no instrumented lines were measured; the coverage run measured nothing")
    else:
        overall = percent(total_covered, total_count)
        verdict = "OK" if overall >= min_overall else "BELOW"
        print(f"[coverage] overall line coverage {overall:.2f}% ({total_covered}/{total_count}), minimum {min_overall:g}%: {verdict}")
        if overall < min_overall:
            failures.append(f"overall line coverage {overall:.2f}% is below {min_overall:g}%")

    entries = []
    for raw in Path(critical_path).read_text().splitlines():
        words = raw.split("#", 1)[0].split()
        if not words:
            continue
        if len(words) > 2:
            print(f"[coverage] {critical_path}: '{raw.strip()}' is not 'PATH [MIN]'", file=sys.stderr)
            return 2
        try:
            minimum = float(words[1]) if len(words) == 2 else min_critical
        except ValueError:
            print(f"[coverage] {critical_path}: minimum '{words[1]}' is not a number", file=sys.stderr)
            return 2
        entries.append((words[0].rstrip("/"), minimum))
    if not entries:
        print(f"[coverage] critical paths: none listed in {critical_path}; the {min_critical:g}% gate has nothing to measure yet")
    for entry, minimum in entries:
        matched = [value for name, value in measured.items() if name == entry or name.startswith(entry + "/")]
        covered = sum(c for c, _ in matched)
        count = sum(n for _, n in matched)
        if not matched or count == 0:
            print(f"[coverage] critical path {entry}: matches no file with instrumented lines: BELOW")
            failures.append(f"critical path {entry} matches no measured file")
            continue
        value = percent(covered, count)
        verdict = "OK" if value >= minimum else "BELOW"
        print(f"[coverage] critical path {entry}: {value:.2f}% ({covered}/{count}), minimum {minimum:g}%: {verdict}")
        if value < minimum:
            failures.append(f"critical path {entry} line coverage {value:.2f}% is below {minimum:g}%")

    for failure in failures:
        print(f"[coverage] FAILED: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

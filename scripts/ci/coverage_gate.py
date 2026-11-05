"""Applies the coverage thresholds to a `cargo llvm-cov report --json --summary-only` file.

usage: coverage_gate.py SUMMARY_JSON WORKSPACE_DIR CRITICAL_PATHS_FILE MIN_OVERALL MIN_CRITICAL
Line coverage is covered/total instrumented lines; crates and files without instrumented lines do not count.
CRITICAL_PATHS_FILE: one path per line, relative to WORKSPACE_DIR (a file or a directory prefix); `#` starts a comment.
Every critical-path entry is measured on its own and must reach MIN_CRITICAL; an entry that matches no file fails.
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
        text = raw.split("#", 1)[0].strip()
        if text:
            entries.append(text.rstrip("/"))
    if not entries:
        print(f"[coverage] critical paths: none listed in {critical_path}; the {min_critical:g}% gate has nothing to measure yet")
    for entry in entries:
        matched = [value for name, value in measured.items() if name == entry or name.startswith(entry + "/")]
        covered = sum(c for c, _ in matched)
        count = sum(n for _, n in matched)
        if not matched or count == 0:
            print(f"[coverage] critical path {entry}: matches no file with instrumented lines: BELOW")
            failures.append(f"critical path {entry} matches no measured file")
            continue
        value = percent(covered, count)
        verdict = "OK" if value >= min_critical else "BELOW"
        print(f"[coverage] critical path {entry}: {value:.2f}% ({covered}/{count}), minimum {min_critical:g}%: {verdict}")
        if value < min_critical:
            failures.append(f"critical path {entry} line coverage {value:.2f}% is below {min_critical:g}%")

    for failure in failures:
        print(f"[coverage] FAILED: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

"""Merges `cargo llvm-cov report --lcov` files of test shards into one `--json --summary-only`-shaped file.

usage: merge_lcov.py OUT_JSON LCOV_FILE...
Each shard ran a part of the tests, so a line's hits are summed over the shards; a line is covered when its sum is
above zero. The output holds the per-file line counts the gate (coverage_gate.py) reads and nothing else.
Lines are the unique lines of the lcov `DA` records. llvm-cov's own `--summary-only` counts them per function (a
closure's lines count again inside its parent), which an lcov file cannot reproduce: on the workspace that is 5-6%
more lines (92.39% there against 93.04% here overall, every critical path within 0.3 points of its value).
Exit 0 written, 1 no input or no instrumented line, 2 usage.
"""

import json
import sys
from pathlib import Path


def read_hits(lcov_path, hits):
    current = None
    for raw in Path(lcov_path).read_text().splitlines():
        if raw.startswith("SF:"):
            current = hits.setdefault(raw[3:], {})
        elif raw.startswith("DA:") and current is not None:
            line, count = raw[3:].split(",")[:2]
            current[int(line)] = current.get(int(line), 0) + int(count)
        elif raw == "end_of_record":
            current = None


def main():
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    out_path, lcov_paths = sys.argv[1], sys.argv[2:]
    hits = {}
    for lcov_path in lcov_paths:
        read_hits(lcov_path, hits)
    files = []
    for name in sorted(hits):
        counts = hits[name]
        covered = sum(1 for count in counts.values() if count > 0)
        if counts:
            files.append({"filename": name, "summary": {"lines": {"count": len(counts), "covered": covered}}})
    if not files:
        print(f"[merge-lcov] no instrumented line in {len(lcov_paths)} lcov files", file=sys.stderr)
        return 1
    Path(out_path).write_text(json.dumps({"data": [{"files": files}]}))
    print(f"[merge-lcov] {len(lcov_paths)} lcov files, {len(files)} files with instrumented lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())

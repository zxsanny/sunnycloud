"""Validates the advisory ignore entries of a cargo-deny configuration and prints their ids.

usage: audit_ignores.py DENY_TOML TODAY(YYYY-MM-DD)
stdout: one advisory id per valid entry. stderr: one line per violation. Exit 1 when any entry is invalid.
Each entry must be { id = "RUSTSEC-...", reason = "added=YYYY-MM-DD; expires=YYYY-MM-DD; why=..." },
not past `expires`, and `added` at most 90 days before TODAY.
"""

import re
import sys
import tomllib
from datetime import date

MAX_AGE_DAYS = 90
REASON = re.compile(r"^\s*added=(\d{4}-\d{2}-\d{2});\s*expires=(\d{4}-\d{2}-\d{2});\s*why=(.+?)\s*$", re.DOTALL)


def check(entry, today):
    if isinstance(entry, str):
        return None, f"{entry}: plain string entry; use {{ id, reason = \"added=...; expires=...; why=...\" }}"
    advisory_id = entry.get("id")
    if not advisory_id:
        return None, f"{entry}: entry has no `id` (crate-only ignores are not supported)"
    match = REASON.match(entry.get("reason", ""))
    if not match:
        return None, f"{advisory_id}: reason must be `added=YYYY-MM-DD; expires=YYYY-MM-DD; why=<text>` (added, expires and why are all required)"
    try:
        added = date.fromisoformat(match.group(1))
        expires = date.fromisoformat(match.group(2))
    except ValueError as error:
        return None, f"{advisory_id}: invalid date ({error})"
    if expires < today:
        return None, f"{advisory_id}: expired on {expires} (today {today})"
    if added > today:
        return None, f"{advisory_id}: added date {added} is in the future"
    age = (today - added).days
    if age > MAX_AGE_DAYS:
        return None, f"{advisory_id}: added {age} days ago, the limit is {MAX_AGE_DAYS}"
    return advisory_id, None


def main():
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    path, today_text = sys.argv[1], sys.argv[2]
    try:
        today = date.fromisoformat(today_text)
    except ValueError as error:
        print(f"invalid TODAY `{today_text}`: {error}", file=sys.stderr)
        return 2
    with open(path, "rb") as handle:
        config = tomllib.load(handle)
    entries = config.get("advisories", {}).get("ignore", [])
    failed = False
    for entry in entries:
        advisory_id, problem = check(entry, today)
        if problem:
            failed = True
            print(f"INVALID IGNORE {problem}", file=sys.stderr)
        else:
            print(advisory_id)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

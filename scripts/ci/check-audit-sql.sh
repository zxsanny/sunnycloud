#!/usr/bin/env bash
# audit_log is append only for the API role (ADR-014, sc_audit contract). This gate fails on any statement against it
# other than the three the audit crate owns:
#   - the writer insert   (INSERT INTO audit_log (...))   in src/shared/audit/src/repo.rs
#   - the export select   (SELECT ... FROM audit_log ...) in src/shared/audit/src/repo.rs
#   - the purge call      (SELECT audit_log_purge(...))   in src/shared/audit/src/repo.rs
# plus the DDL of the migrator's own migrations in src/migrations/: CREATE/ALTER/DROP TABLE, CREATE INDEX, COMMENT,
# REVOKE, GRANT of INSERT and SELECT only (never UPDATE, DELETE, TRUNCATE, ALL or WITH GRANT OPTION), the purge function
# (only in the audit migration: DELETE FROM audit_log WHERE at < cutoff, SECURITY DEFINER with a fixed search_path) and
# its EXECUTE grant (never to PUBLIC).
# Scanned: ROOT/src/**/*.rs and *.sql, except target, .git, node_modules and any tests directory (the privilege tests
# issue UPDATE and DELETE on purpose, as the API role, to see them fail). Full-line comments are ignored.
# usage: check-audit-sql.sh [ROOT]   (default: repository root)
# Exit code: 0 clean, 1 any violation, 2 usage or internal error.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${1:-$(cd "$ci_dir/../.." && pwd)}"
tag="[check-audit-sql]"
[ -d "$root/src" ] || { echo "$tag FAILED: $root/src is not a directory" >&2; exit 2; }
command -v python3 >/dev/null || { echo "$tag FAILED: python3 not found" >&2; exit 2; }

echo "$tag scanning $root/src for statements against audit_log"
exec python3 -I - "$root" <<'PY'
import pathlib
import re
import sys

TAG = "[check-audit-sql]"
root = pathlib.Path(sys.argv[1]).resolve()
SKIP_DIRS = {"target", ".git", "node_modules", "tests"}
REPO = "src/shared/audit/src/repo.rs"
MIGRATIONS = "src/migrations/"
TABLE = re.compile(r"(?<![A-Za-z0-9_])audit_log(?![A-Za-z0-9_])", re.IGNORECASE)
PURGE = re.compile(r"(?<![A-Za-z0-9_])audit_log_purge(?![A-Za-z0-9_])", re.IGNORECASE)
FLAGS = re.IGNORECASE


def flatten(text, suffix):
    keep = []
    for line in text.splitlines():
        stripped = line.strip()
        if suffix == ".rs" and stripped.startswith("//"):
            continue
        if suffix == ".sql":
            line = line.split("--", 1)[0]
        keep.append(line)
    return re.sub(r"\s+", " ", " ".join(keep))


def before_of(flat, start):
    before = flat[max(0, start - 400):start]
    return re.sub(r"public\.$", "", before, flags=FLAGS)


def statement_start(flat, start):
    cut = max(flat.rfind(";", 0, start), flat.rfind("$$", 0, start))
    return flat[cut + 1:start]


def rust_table_ok(rel, flat, match):
    if rel != REPO:
        return False
    before = before_of(flat, match.start())
    if re.search(r"\bINSERT\s+INTO\s+$", before, FLAGS):
        return re.match(r"\s*\\?\s*\(", flat[match.end():match.end() + 20]) is not None
    select = re.search(r"\bSELECT\b([^;\"]*?)\bFROM\s+$", before, FLAGS)
    return bool(select) and not re.search(r"\b(DELETE|UPDATE|INSERT|TRUNCATE)\b", select.group(1), FLAGS)


def rust_purge_ok(rel, flat, match):
    before = before_of(flat, match.start())
    return rel == REPO and re.search(r"\bSELECT\s+$", before, FLAGS) is not None and flat[match.end():match.end() + 1] == "("


def migration_table_ok(rel, flat, match):
    before = before_of(flat, match.start())
    after = flat[match.end():]
    if re.search(r"\b(CREATE\s+TABLE|DROP\s+TABLE\s+(IF\s+EXISTS\s+)?|ALTER\s+TABLE\s+(ONLY\s+|IF\s+EXISTS\s+)?|COMMENT\s+ON\s+TABLE)\s*$", before, FLAGS):
        return True
    if re.search(r"\bDELETE\s+FROM\s+$", before, FLAGS):
        in_audit = "_audit_" in rel.rsplit("/", 1)[-1] and rel.endswith(".up.sql")
        return in_audit and re.match(r"\s*WHERE\s+at\s*<\s*cutoff\s*;", after, FLAGS) is not None
    if re.search(r"\bON\s+$", before, FLAGS):
        stmt = statement_start(flat, match.start())
        if re.match(r"\s*(CREATE\s+(UNIQUE\s+)?INDEX|REVOKE)\b", stmt, FLAGS):
            return True
        grant = re.match(r"\s*GRANT\s+(.*?)\s+ON\s*$", stmt, FLAGS)
        if grant:
            privileges = {p.strip().upper() for p in grant.group(1).split(",")}
            tail = after.split(";", 1)[0]
            return privileges <= {"INSERT", "SELECT"} and not re.search(r"\bGRANT\s+OPTION\b", tail, FLAGS)
    return False


def migration_purge_ok(rel, flat, match):
    before = before_of(flat, match.start())
    after = flat[match.end():]
    if re.search(r"\bDROP\s+FUNCTION\s+(IF\s+EXISTS\s+)?$", before, FLAGS):
        return True
    if re.search(r"\bCREATE\s+(OR\s+REPLACE\s+)?FUNCTION\s+$", before, FLAGS):
        in_audit = "_audit_" in rel.rsplit("/", 1)[-1] and rel.endswith(".up.sql")
        definition = after[:700]
        definer = re.search(r"\bSECURITY\s+DEFINER\b", definition, FLAGS)
        return in_audit and bool(definer) and re.search(r"\bSET\s+search_path\s*=", definition, FLAGS) is not None
    if re.search(r"\bON\s+FUNCTION\s+$", before, FLAGS):
        stmt = statement_start(flat, match.start())
        if re.match(r"\s*REVOKE\b", stmt, FLAGS):
            return True
        tail = after.split(";", 1)[0]
        return re.match(r"\s*GRANT\s+EXECUTE\s+ON\s+FUNCTION\s*$", stmt, FLAGS) is not None and not re.search(r"\bPUBLIC\b", tail, FLAGS)
    return False


violations = []
statements = 0
files = 0
for path in sorted((root / "src").rglob("*")):
    if path.suffix not in (".rs", ".sql") or not path.is_file():
        continue
    rel = path.relative_to(root).as_posix()
    if SKIP_DIRS & set(rel.split("/")):
        continue
    flat = flatten(path.read_text(errors="replace"), path.suffix)
    in_migrations = rel.startswith(MIGRATIONS) and path.suffix == ".sql"
    found = False
    for pattern, kind in ((TABLE, "table"), (PURGE, "purge")):
        for match in pattern.finditer(flat):
            found = True
            statements += 1
            if in_migrations:
                ok = migration_table_ok(rel, flat, match) if kind == "table" else migration_purge_ok(rel, flat, match)
            else:
                ok = rust_table_ok(rel, flat, match) if kind == "table" else rust_purge_ok(rel, flat, match)
            if not ok:
                context = flat[max(0, match.start() - 50):match.end() + 40].strip()
                violations.append(f"{rel}: statement against audit_log is not the writer insert, the export select or the purge call: ...{context}...")
    files += 1 if found else 0

if violations:
    for line in violations:
        print(line, file=sys.stderr)
    print(f"{TAG} FAILED: {len(violations)} statement(s) against audit_log outside the audit crate's insert, export select and purge call", file=sys.stderr)
    sys.exit(1)
print(f"{TAG} OK: {statements} audit_log reference(s) in {files} file(s), all of them the insert, the export select, the purge call or migrator DDL")
PY

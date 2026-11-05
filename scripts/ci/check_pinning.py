"""Pinning check (RESTRICT-30, NFT-SEC-37): prints one line per violation, exits 1 when there is any.

usage: check_pinning.py ROOT
- workflows (.github/workflows/*.yml, .github/actions/**/action.yml): every `uses:` is OWNER/REPO[/PATH]@<40 hex> followed by `# vX.Y.Z`;
  `docker://` uses and workflow `image:` lines need @sha256:<64 hex>; local `./` uses are allowed
- the workflow default `permissions:` is present and is exactly `contents: read` (no write, no other scope)
- a workflow triggered by pull_request_target must not check out the pull request head (fork code)
- Dockerfiles (Dockerfile, *.Dockerfile, Dockerfile.*): every FROM image has @sha256:<64 hex>, except `scratch` and earlier stage names
- compose files (compose*.yml, docker-compose*.yml): every `image:` has @sha256:<64 hex>, except a service that also has `build:`
"""

import re
import sys
from pathlib import Path

SKIP_DIRS = {".git", "target", "node_modules"}
USES = re.compile(r"^\s*(?:-\s+)?uses:\s*(?P<ref>[^\s#]+)(?P<rest>.*)$")
SHA_REF = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_./-]+)?@[0-9a-f]{40}$")
VERSION_COMMENT = re.compile(r"^\s*#\s*v\d+\.\d+\.\d+\S*\s*$")
DIGEST = re.compile(r"@sha256:[0-9a-f]{64}$")
FROM = re.compile(r"^\s*FROM\s+(?:--\S+\s+)*(?P<image>\S+)(?:\s+AS\s+(?P<name>\S+))?\s*$", re.IGNORECASE)
IMAGE_LINE = re.compile(r"^(?P<indent>\s*)(?:-\s+)?image:\s*(?P<value>[^\s#]+)")
FORK_CHECKOUT = re.compile(r"github\.event\.pull_request\.head|github\.head_ref|pull_request\.head\.(?:sha|ref|repo)|head\.repo")


def indent_of(line):
    return len(line) - len(line.lstrip(" "))


def files(root, patterns):
    found = set()
    for pattern in patterns:
        for path in root.glob(pattern):
            if path.is_file() and not (SKIP_DIRS & set(path.relative_to(root).parts)):
                found.add(path)
    return sorted(found)


def check_workflow(path, report):
    lines = path.read_text().splitlines()
    for number, line in enumerate(lines, 1):
        match = USES.match(line)
        if match:
            ref, rest = match.group("ref"), match.group("rest")
            if ref.startswith("./"):
                continue
            if ref.startswith("docker://"):
                if not DIGEST.search(ref):
                    report(path, number, f"docker action not pinned by digest: {ref}")
            elif not SHA_REF.match(ref):
                report(path, number, f"action not pinned to a 40-hex commit SHA: {ref}")
            elif not VERSION_COMMENT.match(rest):
                report(path, number, f"pinned action lacks a `# vX.Y.Z` version comment: {ref}")
            continue
        image = IMAGE_LINE.match(line)
        if image and not DIGEST.search(image.group("value")):
            report(path, number, f"workflow image not pinned by digest: {image.group('value')}")

    check_default_permissions(path, lines, report)

    if any("pull_request_target" in line and not line.lstrip().startswith("#") for line in lines):
        for number, line in enumerate(lines, 1):
            if FORK_CHECKOUT.search(line) and not line.lstrip().startswith("#"):
                report(path, number, "pull_request_target workflow references the pull request head (fork checkout)")


def check_default_permissions(path, lines, report):
    start = next((i for i, line in enumerate(lines) if re.match(r"^permissions:", line)), None)
    if start is None:
        report(path, 1, "no top-level `permissions:`; the default must be `contents: read`")
        return
    first = lines[start]
    inline = first.split(":", 1)[1].strip()
    if inline:
        report(path, start + 1, f"top-level permissions must be the mapping `contents: read`, found `{inline}`")
        return
    scopes = {}
    for line in lines[start + 1 :]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if indent_of(line) == 0:
            break
        key, _, value = line.strip().partition(":")
        scopes[key.strip()] = value.split("#")[0].strip()
    if scopes != {"contents": "read"}:
        report(path, start + 1, f"top-level permissions must be exactly `contents: read`, found {scopes or 'empty'}")


def check_dockerfile(path, report):
    stages = set()
    for number, line in enumerate(path.read_text().splitlines(), 1):
        match = FROM.match(line)
        if not match:
            continue
        image = match.group("image")
        if image != "scratch" and image not in stages and not DIGEST.search(image):
            report(path, number, f"base image not pinned by digest: {image}")
        if match.group("name"):
            stages.add(match.group("name"))


def check_compose(path, report):
    lines = path.read_text().splitlines()
    for number, line in enumerate(lines, 1):
        match = IMAGE_LINE.match(line)
        if not match or DIGEST.search(match.group("value")):
            continue
        indent = len(match.group("indent"))
        if line.lstrip().startswith("- "):
            indent += 2
        if has_sibling_build(lines, number - 1, indent):
            continue
        report(path, number, f"compose image not pinned by digest: {match.group('value')}")


def has_sibling_build(lines, index, indent):
    for step in (-1, 1):
        cursor = index + step
        while 0 <= cursor < len(lines):
            line = lines[cursor]
            if line.strip() and indent_of(line) < indent:
                break
            if indent_of(line) == indent and re.match(r"\s*build:", line):
                return True
            cursor += step
    return False


def main():
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = Path(sys.argv[1]).resolve()
    violations = []

    def report(path, number, message):
        violations.append(f"{path.relative_to(root)}:{number}: {message}")

    workflows = files(root, [".github/workflows/*.yml", ".github/workflows/*.yaml", ".github/actions/**/action.yml"])
    dockerfiles = files(root, ["**/Dockerfile", "**/*.Dockerfile", "**/Dockerfile.*"])
    composes = files(root, ["**/compose*.yml", "**/compose*.yaml", "**/docker-compose*.yml"])
    if not workflows:
        violations.append(".github/workflows: no workflow files found")
    for path in workflows:
        check_workflow(path, report)
    for path in dockerfiles:
        check_dockerfile(path, report)
    for path in composes:
        check_compose(path, report)

    print(f"checked {len(workflows)} workflow(s), {len(dockerfiles)} Dockerfile(s), {len(composes)} compose file(s)")
    for violation in violations:
        print(f"UNPINNED {violation}")
    print(f"{len(violations)} unpinned or unsafe reference(s)")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())

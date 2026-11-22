"""JSON helpers of check-decoder-image.sh (AZ-3864).

usage: decoder_image_inspect.py INSPECT_JSON                    print `key=value` lines of a `docker image inspect` file
       decoder_image_inspect.py --sbom FILE NAME=VERSION...     exit 1 and print the reason unless the CycloneDX file lists
                                                                every named component with that version and a CPE for it
       decoder_image_inspect.py --facts FILE NAME=VERSION...    the same for the build facts file, which also needs a
                                                                build_timestamp in Unix seconds that is not in the future
"""

import json
import sys
import time


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def print_config(path):
    image = load(path)[0]
    config = image["Config"]
    print(f"user={config.get('User', '')}")
    print(f"entrypoint={json.dumps(config.get('Entrypoint') or [], separators=(',', ':'))}")
    print(f"size={image['Size']}")
    for key, value in sorted((config.get("Labels") or {}).items()):
        print(f"label.{key}={value}")


def parse_wanted(items):
    return dict(item.split("=", 1) for item in items)


def check_sbom(path, wanted):
    try:
        sbom = load(path)
    except ValueError as error:
        return [f"components.cdx.json is not valid JSON: {error}"]
    problems = []
    if sbom.get("bomFormat") != "CycloneDX":
        problems.append("bomFormat is not CycloneDX")
    components = {c.get("name"): c for c in sbom.get("components", [])}
    for name, version in wanted.items():
        component = components.get(name)
        if component is None:
            problems.append(f"component {name} is not listed")
            continue
        if component.get("version") != version:
            problems.append(f"{name} version is {component.get('version')!r}, want {version!r}")
        cpe = component.get("cpe", "")
        if not cpe.startswith("cpe:2.3:a:") or f":{version}:" not in cpe:
            problems.append(f"{name} has no CPE for version {version}: {cpe!r}")
    return problems


def check_facts(path, wanted):
    try:
        facts = load(path)
    except ValueError as error:
        return [f"build-facts.json is not valid JSON: {error}"]
    problems = []
    for name, version in wanted.items():
        if facts.get(name) != version:
            problems.append(f"{name} is {facts.get(name)!r} in the build facts, want {version!r}")
    stamp = facts.get("build_timestamp")
    if not isinstance(stamp, int) or isinstance(stamp, bool) or stamp < 1:
        problems.append(f"build_timestamp {stamp!r} is not Unix seconds")
    elif stamp > time.time() + 60:
        problems.append(f"build_timestamp {stamp} is in the future")
    return problems


def main(argv):
    if len(argv) == 2 and not argv[1].startswith("--"):
        print_config(argv[1])
        return 0
    if len(argv) >= 3 and argv[1] in ("--sbom", "--facts"):
        check = check_sbom if argv[1] == "--sbom" else check_facts
        problems = check(argv[2], parse_wanted(argv[3:]))
        if problems:
            print("; ".join(problems))
            return 1
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))

"""AC-3 (layer part): fail when a saved image holds a path or content that must never ship.

usage: scan_layers.py IMAGE.tar [IMAGE.tar ...]   (output of `docker save`)
Markers are the strings planted by the test: AZ3741_PLANTED_ENV_MARKER and AZ3741_PLANTED_SECRET_MARKER.
"""
import io
import re
import sys
import tarfile

MARKERS = (b"AZ3741_PLANTED_ENV_MARKER", b"AZ3741_PLANTED_SECRET_MARKER")
BAD_PATH = re.compile(r"(^|/)(\.env[^/]*|secrets[^/]*|_docs|target|node_modules|e2e-results|\.git)(/|$)")


def scan_layer(layer, label):
    problems = 0
    for member in layer:
        if BAD_PATH.search(member.name):
            print(f"FAIL {label}: path {member.name}")
            problems += 1
        if member.isfile() and member.size < 50_000_000:
            data = layer.extractfile(member).read()
            for marker in MARKERS:
                if marker in data:
                    print(f"FAIL {label}: marker {marker.decode()} in {member.name}")
                    problems += 1
    return problems


def main(paths):
    problems = 0
    layers = 0
    for path in paths:
        with tarfile.open(path) as image:
            for member in image:
                if not member.isfile():
                    continue
                raw = image.extractfile(member).read()
                try:
                    layer = tarfile.open(fileobj=io.BytesIO(raw), mode="r:*")
                except tarfile.ReadError:
                    continue  # manifest, config or index json: not a layer
                with layer:
                    layers += 1
                    problems += scan_layer(layer, f"{path}:{member.name}")
    print(f"scanned {layers} layer(s) in {len(paths)} image archive(s)")
    return 1 if problems or layers == 0 else 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(2)
    sys.exit(main(sys.argv[1:]))

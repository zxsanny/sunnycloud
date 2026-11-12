#!/usr/bin/env bash
# Fails when a decoder-runner binary carries the test-only freeze trigger (10_decoder description
# section 3). Usage: check-no-test-freeze.sh <path to decoder-runner>
#   exit 0  no trigger found (a release binary must give this)
#   exit 1  trigger found    (a binary built with --features test-freeze gives this)
#   exit 2  usage or tool error
set -euo pipefail

binary="${1:?usage: check-no-test-freeze.sh <decoder-runner binary>}"
[ -f "$binary" ] || { echo "not a file: $binary" >&2; exit 2; }

found=0
if marker=$(strings -a "$binary" | grep -o -e 'test_freeze_main_loop' -e 'test_freeze' | sort | uniq -c); then
    echo "FOUND in strings:"
    echo "$marker"
    found=1
fi
# A stripped release binary has no symbol table; nm then exits non-zero and strings decides.
if symbols=$(nm -C "$binary" 2>&1); then
    if hits=$(echo "$symbols" | grep -e 'test_freeze'); then
        echo "FOUND in symbols:"
        echo "$hits" | head -5
        found=1
    fi
else
    echo "nm: no symbol table ($symbols); strings result stands" >&2
fi
if [ "$found" -eq 1 ]; then
    echo "test-freeze trigger present in $binary"
    exit 1
fi
echo "no test-freeze trigger in $binary"

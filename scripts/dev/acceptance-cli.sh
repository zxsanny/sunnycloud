#!/usr/bin/env bash
# Runs the `sunnycloud` CLI against the acceptance stack: scripts/dev/acceptance-cli.sh admin setup-token
set -euo pipefail
# shellcheck source=acceptance-lib.sh
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"
load_env
exec "$(binary_of api)" "$@"

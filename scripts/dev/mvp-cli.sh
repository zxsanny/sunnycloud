#!/usr/bin/env bash
# Runs the `sunnycloud` CLI against the MVP stack with its environment: scripts/dev/mvp-cli.sh doctor,
# scripts/dev/mvp-cli.sh admin setup-token, scripts/dev/mvp-cli.sh admin invite create ...
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"
load_env
exec "$(binary_of api)" "$@"

#!/usr/bin/env bash
# Stops the supervised host processes (their systemd --user units, so nothing restarts them) and the Docker
# services.
# Volumes (database, certificates) and data under sunnycloud-data are kept.
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"
require_systemd_user

# Reverse start order: the worker and the decoders before the API they talk to.
for name in worker api decoder-general decoder-interactive; do
  stop_process "$name"
done
compose down
log "down"

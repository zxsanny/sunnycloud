#!/usr/bin/env bash
# Removes the acceptance stack: its systemd --user units, the database container (tmpfs, so the data goes with
# it) and the data directory with the secrets and logs. The cargo target and the web build copy under
# /develop/zxsanny/targets/acceptance are a build cache and stay; ACC_PURGE_BUILD=1 removes them too.
# The MVP stack is not touched: every name here starts with sunnycloud-acc.
set -euo pipefail
# shellcheck source=acceptance-lib.sh
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"
require_systemd_user

while IFS= read -r name; do
  stop_process "$name"
done < <(all_units_stop_order)
docker rm -f -v "$ACC_DB_CONTAINER" > /dev/null 2>&1 || log "no database container to remove"
[ -z "$(docker ps -aq --filter "name=^$ACC_DB_CONTAINER\$")" ] || die "$ACC_DB_CONTAINER is still there"
rm -rf -- "$ACC_DATA"
if [ "${ACC_PURGE_BUILD:-}" = 1 ]; then
  [ "$(basename "$ACC_TARGET")" = acceptance ] || die "ACC_TARGET must end in /acceptance to be purged"
  rm -rf -- "$ACC_TARGET"
fi
log "down: no units, no container, no data directory"

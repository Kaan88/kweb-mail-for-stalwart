#!/usr/bin/env bash
# Starts the local Stalwart test server in the background (plain HTTP, 127.0.0.1:18080).
# Port 18080 on purpose: it stays clear of anything on 8080. Stop it with dev/stalwart-stop.sh.
# Extra environment, such as STALWART_RECOVERY_MODE=1, is passed through.
set -euo pipefail
ROOT="${KWEB_TOOLS:-$HOME/tools}/stalwart-test"
cd "$ROOT"
if [ -f etc/stalwart.pid ] && kill -0 "$(cat etc/stalwart.pid)" 2>/dev/null; then
  echo "already running (pid $(cat etc/stalwart.pid))"; exit 0
fi
export STALWART_RECOVERY_ADMIN="$(cat etc/admin.txt)"
export STALWART_PUBLIC_URL="http://127.0.0.1:18080"
export STALWART_HOSTNAME="127.0.0.1"
export STALWART_RECOVERY_MODE_PORT=18080
nohup ./bin/stalwart --config=etc/config.json >> logs/stalwart.out 2>&1 &
echo $! > etc/stalwart.pid
for _ in $(seq 1 60); do
  curl -s -o /dev/null http://127.0.0.1:18080/jmap/session && { echo "up on http://127.0.0.1:18080 (pid $!)"; exit 0; }
  sleep 0.5
done
echo "did not answer within 30 s; see $ROOT/logs/stalwart.out" >&2; exit 1

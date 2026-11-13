#!/usr/bin/env bash
# One-time set-up of a fresh local Stalwart test server: first-time bootstrap, the HTTP listener,
# the domain kweb.test and the user tester@kweb.test. Needs ~/tools/stalwart-test/{bin/stalwart,node}
# (see dev/README.md) and does nothing if etc/config.json already exists.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${KWEB_TOOLS:-$HOME/tools}/stalwart-test"
cd "$ROOT"
export PATH="$ROOT/node/bin:$PATH"
[ -e etc/config.json ] && { echo "already set up ($ROOT/etc/config.json exists)"; exit 0; }
[ -f etc/admin.txt ] || { python3 -c "import secrets;print('admin:'+secrets.token_urlsafe(18), end='')" > etc/admin.txt; chmod 600 etc/admin.txt; }
mkdir -p data logs

export STALWART_URL=http://127.0.0.1:18080 STALWART_USER=admin
export STALWART_PASSWORD="$(cut -d: -f2- etc/admin.txt)"

"$HERE/stalwart-run.sh"
# 1. First-time set-up (bootstrap mode). The reply holds the generated administrator login.
reply="$(stalwart-cli --no-color update Bootstrap singleton --json "{
  \"defaultHostname\":\"localhost\",\"defaultDomain\":\"kweb.test\",
  \"requestTlsCertificate\":false,\"generateDkimKeys\":false,
  \"dataStore\":{\"@type\":\"RocksDb\",\"path\":\"$ROOT/data/\",\"blobSize\":4096,\"bufferSize\":134217728,\"poolWorkers\":null,\"cacheSize\":null},
  \"tracer\":{\"@type\":\"Log\",\"path\":\"$ROOT/logs/\",\"prefix\":\"stalwart.log\",\"rotate\":\"daily\",\"ansi\":false,\"multiline\":false,\"enable\":true,\"level\":\"info\",\"lossy\":false,\"events\":{},\"eventsPolicy\":\"exclude\"}}")"
user="$(printf '%s\n' "$reply" | sed -n 's/.*username: "\(.*\)"/\1/p')"
secret="$(printf '%s\n' "$reply" | sed -n 's/.*secret: "\(.*\)"/\1/p')"
printf '%s:%s' "$user" "$secret" > etc/admin-account.txt; chmod 600 etc/admin-account.txt

# 2. Restart in recovery mode: the full management API answers, as the recovery administrator.
"$HERE/stalwart-stop.sh"
STALWART_RECOVERY_MODE=1 "$HERE/stalwart-run.sh"

# 3. The listener the normal start will use, and the user to sign in as (domain id b = kweb.test).
stalwart-cli --no-color create NetworkListener --json \
  '{"name":"http","protocol":"http","bind":{"127.0.0.1:18080":true},"useTls":false,"implicitTls":false}'
python3 -c "import secrets;print('tester@kweb.test:'+secrets.token_urlsafe(12), end='')" > etc/tester.txt; chmod 600 etc/tester.txt
stalwart-cli --no-color create Account/User --field name=tester --field domainId=b \
  --field 'roles={"@type":"User"}' --field description="Test user" \
  --field "credentials={\"0\":{\"@type\":\"Password\",\"secret\":\"$(cut -d: -f2- etc/tester.txt)\"}}"

# 4. Normal start.
"$HERE/stalwart-stop.sh"
"$HERE/stalwart-run.sh"
echo "set up. Sign in as: $(cut -d: -f1 etc/tester.txt) (password in $ROOT/etc/tester.txt)"

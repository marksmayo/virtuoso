#!/bin/bash
# Serves the built web benchmark app (bench/web/build-app.sh) on port 4280 in the background, and
# returns once it answers. Its process id goes to bench/web/app/server.pid.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/app"
NEXT_TELEMETRY_DISABLED=1 nohup npx next start app --port 4280 > server.log 2>&1 &
echo $! > server.pid
for _ in $(seq 1 120); do
  if curl -fsS -o /dev/null http://127.0.0.1:4280/; then
    echo "web benchmark app on http://127.0.0.1:4280"
    exit 0
  fi
  sleep 0.5
done
echo "the web benchmark app did not answer within 60 s:" >&2
cat server.log >&2
exit 1

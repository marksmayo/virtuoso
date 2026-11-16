#!/bin/bash
# Tester Army's web benchmark app (tester-army/e2e, apps/web-benchmark/app), built to serve: the
# app alone, at the commit the mobile benchmark pins, with the versions its package.json names
# (the monorepo's own packages are its test suites', not the app's). Into bench/web/app; then
# `bench/web/serve-app.sh` serves it on port 4280, as upstream's `next start` does.
# Usage: bench/web/build-app.sh [commit]
set -euo pipefail
commit="${1:-${TAM_COMMIT:-f4be1c456858b6c8bd4839053f5e267f75c937d8}}"
here="$(cd "$(dirname "$0")" && pwd)"
dir="$here/app"
if [ -f "$dir/.built-$commit" ]; then
  echo "web benchmark app at $commit: built already"
  exit 0
fi
rm -rf "$dir"
mkdir -p "$dir"
cd "$dir"
git init -q
git remote add origin https://github.com/tester-army/e2e.git
git config core.sparseCheckout true
echo "apps/web-benchmark/app/" > .git/info/sparse-checkout
git fetch -q --depth 1 --filter=blob:none origin "$commit"
git checkout -q FETCH_HEAD
mv apps/web-benchmark/app ./app
rm -rf apps .git
cat > package.json <<'JSON'
{
  "name": "web-benchmark-app",
  "private": true,
  "type": "module",
  "scripts": { "build": "next build app", "start": "next start app --port 4280" },
  "dependencies": { "next": "16.4.4", "react": "19.3.1", "react-dom": "19.3.1" },
  "devDependencies": { "@types/node": "26.6.6", "@types/react": "19.3.0", "@types/react-dom": "19.3.0", "typescript": "7.0.2" }
}
JSON
npm install --no-audit --no-fund --loglevel=error
NEXT_TELEMETRY_DISABLED=1 npx next build app
touch ".built-$commit"
echo "web benchmark app at $commit: built in $dir"

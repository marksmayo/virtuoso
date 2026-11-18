#!/bin/bash
# Sets up a web benchmark job (web-bench.yml) on any of GitHub's runners (bash; Git Bash on
# Windows): the code tools' packages (bench/web/<tool>, from their lockfiles), Maestro 2.11.0
# from its release zip, and Tester Army's web benchmark app, built and served on port 4280.
# The tools set up are those of BENCH_TOOLS (comma-separated); all when it is empty.
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
tools="$repo/bench/tools"
wanted() { [ -z "${BENCH_TOOLS:-}" ] || [[ ",$BENCH_TOOLS," == *",$1,"* ]]; }
get() { curl -fsSL --retry 5 --retry-delay 5 --retry-all-errors "$@"; }

pids=()
# The Android field (VIRTUOSO_BENCH_WEB_DEVICE=android): no Cypress (it drives no device), and
# Appium (the mobile benchmark's install: its server and UiAutomator2 driver).
android=""; [ "${VIRTUOSO_BENCH_WEB_DEVICE:-}" = android ] && android=1
for t in playwright puppeteer selenium cypress e2e; do
  [ -n "$android" ] && [ "$t" = cypress ] && continue
  if wanted "$t"; then
    (cd "$repo/bench/web/$t" && npm ci --no-audit --no-fund --loglevel=error && echo "$t: $(node run.mjs --version)") &
    pids+=("$!")
  fi
done
if [ -n "$android" ] && wanted appium; then
  (
    cd "$repo/bench/appium" && npm ci --no-audit --no-fund --loglevel=error
    APPIUM_HOME="$HOME/.appium" npx --prefix "$repo/bench/appium" appium driver install uiautomator2@8.0.0
  ) &
  pids+=("$!")
fi
if wanted maestro && [ ! -d "$tools/maestro-2.11.0" ]; then
  (
    get -o "${RUNNER_TEMP:-/tmp}/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
    mkdir -p "$tools/maestro-2.11.0"
    unzip -q -o "${RUNNER_TEMP:-/tmp}/maestro.zip" -d "$tools/maestro-2.11.0"
  ) &
  pids+=("$!")
fi
bash "$repo/bench/web/build-app.sh"
for p in "${pids[@]}"; do wait "$p"; done
bash "$repo/bench/web/serve-app.sh"

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
# A Firefox field (VIRTUOSO_BENCH_WEB_BROWSER=firefox, bench/web/FIREFOX.md): no Maestro (its web
# mode drives Chrome only); geckodriver for Selenium, pinned (GECKODRIVER_VERSION), its path in
# VIRTUOSO_GECKODRIVER; Playwright's own Firefox for Playwright and e2e, below. The Firefox the
# other tools drive is the workflow's (VIRTUOSO_FIREFOX).
firefox=""; [ "${VIRTUOSO_BENCH_WEB_BROWSER:-chrome}" = firefox ] && firefox=1
if [ -n "$firefox" ] && wanted selenium; then
  gecko="${GECKODRIVER_VERSION:-0.37.1}"
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset=linux64.tar.gz ;;
    Linux-aarch64) asset=linux-aarch64.tar.gz ;;
    Darwin-arm64) asset=macos-aarch64.tar.gz ;;
    Darwin-*) asset=macos.tar.gz ;;
    *) asset=win64.zip ;;
  esac
  dir="$tools/geckodriver-$gecko"
  mkdir -p "$dir"
  get -o "$dir/$asset" "https://github.com/mozilla/geckodriver/releases/download/v$gecko/geckodriver-v$gecko-$asset"
  case "$asset" in
    *.zip) unzip -q -o "$dir/$asset" -d "$dir" ;;
    *) tar -xzf "$dir/$asset" -C "$dir" ;;
  esac
  exe=""; [ "$asset" = win64.zip ] && exe=.exe
  driver="$dir/geckodriver$exe"
  echo "geckodriver: $("$driver" --version | head -1)"
  # (On Windows as Node.js reads a path: C:/..., not Git Bash's /c/....)
  if command -v cygpath > /dev/null; then driver="$(cygpath -m "$driver")"; fi
  if [ -n "${GITHUB_ENV:-}" ]; then echo "VIRTUOSO_GECKODRIVER=$driver" >> "$GITHUB_ENV"; fi
fi
if [ -z "$firefox" ] && wanted maestro && [ ! -d "$tools/maestro-2.11.0" ]; then
  (
    get -o "${RUNNER_TEMP:-/tmp}/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
    mkdir -p "$tools/maestro-2.11.0"
    unzip -q -o "${RUNNER_TEMP:-/tmp}/maestro.zip" -d "$tools/maestro-2.11.0"
  ) &
  pids+=("$!")
fi
bash "$repo/bench/web/build-app.sh"
# Expanded only when set: macOS's bash 3.2 calls an empty array unbound under `set -u` (a run of
# virtuoso alone starts no setup).
for p in ${pids[@]+"${pids[@]}"}; do wait "$p"; done
# Playwright's Firefox for Playwright and e2e, each its playwright-core's (today both the same
# build: the second install finds it there), with its system libraries on Linux.
if [ -n "$firefox" ]; then
  deps=""
  if [ "$(uname -s)" = Linux ]; then deps=--with-deps; fi
  for t in playwright e2e; do
    if wanted "$t"; then
      (cd "$repo/bench/web/$t" && npx --no-install playwright-core install $deps firefox)
    fi
  done
fi
bash "$repo/bench/web/serve-app.sh"

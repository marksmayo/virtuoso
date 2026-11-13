#!/bin/bash
# The competitors on a Linux runner, at the versions the benchmark pins, with what each needs
# in the environment ($GITHUB_ENV). Run from the repository. Only the tools in $BENCH_TOOLS
# (comma-separated; all when unset) are set up, each alongside the others: independent
# downloads and installs, one after another they left the runner idle.
set -euo pipefail
tools="$PWD/bench/tools"
mkdir -p "$tools"
env_out="${GITHUB_ENV:-/dev/stdout}"
wanted() { [ -z "${BENCH_TOOLS:-}" ] || [[ ",${BENCH_TOOLS// /}," == *",$1,"* ]]; }
# The three release downloads are kept in the workflow's cache (bench/tools): GitHub's release
# downloads time out on and off, and a job then needs none of them.
get() { curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors "$@"; }
# A committed lockfile pins a tool's dependency tree: npm ci installs exactly it, from npm's
# cache. e2e's and Argent's are not committed: npm install for those.
npm_ci() {
  if [ -f "$1/package-lock.json" ] && git ls-files --error-unmatch "$1/package-lock.json" >/dev/null 2>&1; then
    (cd "$1" && npm ci --no-audit --no-fund --loglevel=error)
  else
    (cd "$1" && npm install --no-audit --no-fund --loglevel=error)
  fi
}

# Maestro 2.11.0: its release zip (bench/tools/maestro-2.11.0/maestro, as the harness expects).
install_maestro() {
  [ -d "$tools/maestro-2.11.0" ] && return
  get -o "$RUNNER_TEMP/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
  mkdir -p "$tools/maestro-2.11.0"
  unzip -q -o "$RUNNER_TEMP/maestro.zip" -d "$tools/maestro-2.11.0"
}

# maestro-runner 1.1.29: the binary and its drivers.
install_maestro_runner() {
  [ -d "$tools/maestro-runner-dist" ] && return
  mkdir -p "$tools/maestro-runner-dist"
  get https://github.com/devicelab-dev/maestro-runner/releases/download/v1.1.29/maestro-runner-1.1.29-linux-amd64.tar.gz | tar xz -C "$tools/maestro-runner-dist"
}

# speedrun 0.2.11.
install_speedrun() {
  [ -d "$tools/speedrun" ] && return
  mkdir -p "$tools/speedrun"
  get https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-linux-x86_64.tar.gz | tar xz -C "$tools/speedrun"
}

# Appium 3.9.0 with UiAutomator2 8.0.0. The driver from the workflow's cache when it has it: a
# copy of APPIUM_HOME made straight after its install, before any run wrote there.
install_appium() {
  local cache="$RUNNER_TEMP/appium-home"
  npm_ci bench/appium
  if [ -d "$cache/node_modules/appium-uiautomator2-driver" ]; then
    mkdir -p "$HOME/.appium"
    cp -a "$cache/." "$HOME/.appium/"
  else
    APPIUM_HOME="$HOME/.appium" npx --prefix bench/appium appium driver install uiautomator2@8.0.0
    mkdir -p "$cache"
    cp -a "$HOME/.appium/." "$cache/"
  fi
}

# Each wanted tool in the background; any one failing fails the setup.
pids=()
if wanted maestro; then install_maestro & pids+=("$!"); fi
if wanted maestro-runner; then install_maestro_runner & pids+=("$!"); fi
if wanted speedrun; then install_speedrun & pids+=("$!"); fi
if wanted appium; then install_appium & pids+=("$!"); fi
# e2e and Argent at their pinned versions.
if wanted e2e; then npm_ci bench/e2e & pids+=("$!"); fi
if wanted argent; then npm_ci bench/argent & pids+=("$!"); fi
for pid in "${pids[@]}"; do wait "$pid"; done

if wanted maestro-runner; then
  mr=$(find "$tools/maestro-runner-dist" -type f -name maestro-runner | head -1)
  chmod +x "$mr"
  mr_home=$(dirname "$(find "$tools/maestro-runner-dist" -type d -name drivers | head -1)")
  echo "MAESTRO_RUNNER_BIN=$mr" >> "$env_out"
  echo "MAESTRO_RUNNER_HOME=$mr_home" >> "$env_out"
  echo "maestro-runner: $mr (home $mr_home)"
fi
if wanted speedrun; then
  sr=$(find "$tools/speedrun" -type f -name speedrun | head -1)
  chmod +x "$sr"
  echo "SPEEDRUN_BIN=$sr" >> "$env_out"
  echo "speedrun: $sr"
fi
if wanted appium; then echo "APPIUM_HOME=$HOME/.appium" >> "$env_out"; fi

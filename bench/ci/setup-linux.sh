#!/bin/bash
# The competitors on a Linux runner, at the versions the benchmark pins, with what each needs
# in the environment ($GITHUB_ENV). Run from the repository.
set -euo pipefail
tools="$PWD/bench/tools"
mkdir -p "$tools"
env_out="${GITHUB_ENV:-/dev/stdout}"

# Maestro 2.11.0: its release zip (bench/tools/maestro-2.11.0/maestro, as the harness expects).
curl -fsSL -o "$RUNNER_TEMP/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
mkdir -p "$tools/maestro-2.11.0"
unzip -q -o "$RUNNER_TEMP/maestro.zip" -d "$tools/maestro-2.11.0"

# maestro-runner 1.1.29: the binary and its drivers.
mkdir -p "$tools/maestro-runner-dist"
curl -fsSL https://github.com/devicelab-dev/maestro-runner/releases/download/v1.1.29/maestro-runner-1.1.29-linux-amd64.tar.gz | tar xz -C "$tools/maestro-runner-dist"
mr=$(find "$tools/maestro-runner-dist" -type f -name maestro-runner | head -1)
chmod +x "$mr"
mr_home=$(dirname "$(find "$tools/maestro-runner-dist" -type d -name drivers | head -1)")
echo "MAESTRO_RUNNER_BIN=$mr" >> "$env_out"
echo "MAESTRO_RUNNER_HOME=$mr_home" >> "$env_out"

# speedrun 0.2.11.
mkdir -p "$tools/speedrun"
curl -fsSL https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-linux-x86_64.tar.gz | tar xz -C "$tools/speedrun"
sr=$(find "$tools/speedrun" -type f -name speedrun | head -1)
chmod +x "$sr"
echo "SPEEDRUN_BIN=$sr" >> "$env_out"

# Appium 3.9.0 with UiAutomator2 8.0.0; e2e and Argent at their pinned versions.
(cd bench/appium && npm install --no-audit --no-fund --loglevel=error)
APPIUM_HOME="$HOME/.appium" npx --prefix bench/appium appium driver install uiautomator2@8.0.0
echo "APPIUM_HOME=$HOME/.appium" >> "$env_out"
(cd bench/e2e && npm install --no-audit --no-fund --loglevel=error)
(cd bench/argent && npm install --no-audit --no-fund --loglevel=error)

echo "maestro-runner: $mr (home $mr_home); speedrun: $sr"

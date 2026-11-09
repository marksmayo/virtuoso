#!/bin/bash
# The competitors on a macOS runner for the iOS benchmark, at the versions the benchmark pins,
# with what each needs in the environment ($GITHUB_ENV). Run from the repository.
set -euo pipefail
tools="$PWD/bench/tools"
mkdir -p "$tools"
env_out="${GITHUB_ENV:-/dev/stdout}"

# Maestro 2.11.0 (it drives iOS simulators through its own XCTest runner).
curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors -o "$RUNNER_TEMP/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
mkdir -p "$tools/maestro-2.11.0"
unzip -q -o "$RUNNER_TEMP/maestro.zip" -d "$tools/maestro-2.11.0"

# maestro-runner 1.1.29 for Apple Silicon.
mkdir -p "$tools/maestro-runner-dist"
curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors https://github.com/devicelab-dev/maestro-runner/releases/download/v1.1.29/maestro-runner-1.1.29-darwin-arm64.tar.gz | tar xz -C "$tools/maestro-runner-dist"
mr=$(find "$tools/maestro-runner-dist" -type f -name maestro-runner | head -1)
chmod +x "$mr"
mr_home=$(dirname "$(find "$tools/maestro-runner-dist" -type d -name drivers | head -1)")
echo "MAESTRO_RUNNER_BIN=$mr" >> "$env_out"
echo "MAESTRO_RUNNER_HOME=$mr_home" >> "$env_out"

# speedrun 0.2.11 for Apple Silicon.
mkdir -p "$tools/speedrun"
curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-macos-arm64.tar.gz | tar xz -C "$tools/speedrun"
sr=$(find "$tools/speedrun" -type f -name speedrun | head -1)
chmod +x "$sr"
echo "SPEEDRUN_BIN=$sr" >> "$env_out"

# e2e (Tester Army) and Argent (Software Mansion) at their pinned versions.
(cd bench/e2e && npm install --no-audit --no-fund --loglevel=error)
(cd bench/argent && npm install --no-audit --no-fund --loglevel=error)

# Appium 3.9.0 with its XCUITest driver (WebDriverAgent builds on the first session).
(cd bench/appium && npm install --no-audit --no-fund --loglevel=error)
APPIUM_HOME="$HOME/.appium" npx --prefix bench/appium appium driver install xcuitest
# Its screen recording on iOS (the Speedrun Bench app's appearance flow) runs ffmpeg.
HOMEBREW_NO_AUTO_UPDATE=1 brew install --quiet ffmpeg
echo "APPIUM_HOME=$HOME/.appium" >> "$env_out"

echo "maestro-runner: $mr (home $mr_home); speedrun: $sr"

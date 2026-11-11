#!/bin/bash
# The competitors on a macOS runner for the iOS benchmark, at the versions the benchmark pins,
# with what each needs in the environment ($GITHUB_ENV). Run from the repository. Only the
# tools in $BENCH_TOOLS (comma-separated; all when unset) are set up: Appium's driver and
# ffmpeg, and two npm installs, take minutes a job.
set -euo pipefail
tools="$PWD/bench/tools"
mkdir -p "$tools"
env_out="${GITHUB_ENV:-/dev/stdout}"
wanted() { [ -z "${BENCH_TOOLS:-}" ] || [[ ",$BENCH_TOOLS," == *",$1,"* ]]; }
# The three release downloads are kept in the workflow's cache (bench/tools): GitHub's release
# downloads time out on and off, and a job then needs none of them.
get() { curl -fsSL --retry 10 --retry-delay 15 --retry-all-errors "$@"; }
# Lockfiles pin each tool's dependency tree: npm ci installs exactly it, from npm's cache.
npm_ci() { (cd "$1" && npm ci --no-audit --no-fund --loglevel=error); }
brew_install() { HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK=1 brew install --quiet "$@"; }

# Maestro 2.11.0 (it drives iOS simulators through its own XCTest runner).
if wanted maestro && [ ! -d "$tools/maestro-2.11.0" ]; then
  get -o "$RUNNER_TEMP/maestro.zip" https://github.com/mobile-dev-inc/maestro/releases/download/cli-2.11.0/maestro.zip
  mkdir -p "$tools/maestro-2.11.0"
  unzip -q -o "$RUNNER_TEMP/maestro.zip" -d "$tools/maestro-2.11.0"
fi

# maestro-runner 1.1.29 for Apple Silicon.
if wanted maestro-runner; then
  if [ ! -d "$tools/maestro-runner-dist" ]; then
    mkdir -p "$tools/maestro-runner-dist"
    get https://github.com/devicelab-dev/maestro-runner/releases/download/v1.1.29/maestro-runner-1.1.29-darwin-arm64.tar.gz | tar xz -C "$tools/maestro-runner-dist"
  fi
  mr=$(find "$tools/maestro-runner-dist" -type f -name maestro-runner | head -1)
  chmod +x "$mr"
  mr_home=$(dirname "$(find "$tools/maestro-runner-dist" -type d -name drivers | head -1)")
  echo "MAESTRO_RUNNER_BIN=$mr" >> "$env_out"
  echo "MAESTRO_RUNNER_HOME=$mr_home" >> "$env_out"
  echo "maestro-runner: $mr (home $mr_home)"
fi

# speedrun 0.2.11 for Apple Silicon.
if wanted speedrun; then
  if [ ! -d "$tools/speedrun" ]; then
    mkdir -p "$tools/speedrun"
    get https://github.com/hubastard/speedrun/releases/download/v0.2.11/speedrun-v0.2.11-macos-arm64.tar.gz | tar xz -C "$tools/speedrun"
  fi
  sr=$(find "$tools/speedrun" -type f -name speedrun | head -1)
  chmod +x "$sr"
  echo "SPEEDRUN_BIN=$sr" >> "$env_out"
  echo "speedrun: $sr"
fi

# e2e (Tester Army) and Argent (Software Mansion) at their pinned versions.
if wanted e2e; then npm_ci bench/e2e; fi
if wanted argent; then npm_ci bench/argent; fi

# Appium 3.9.0 with its XCUITest driver (WebDriverAgent builds on the first session).
if wanted appium; then
  npm_ci bench/appium
  APPIUM_HOME="$HOME/.appium" npx --prefix bench/appium appium driver install xcuitest
  # Its screen recording on iOS (the Speedrun Bench app's appearance flow) runs ffmpeg.
  brew_install ffmpeg
  echo "APPIUM_HOME=$HOME/.appium" >> "$env_out"
fi

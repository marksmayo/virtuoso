#!/bin/bash
# One CI job's share of the Android benchmark, on the emulator the job booted: the benchmark
# apps installed, then every tool on the scenarios given ($1), $2 recorded runs each (default
# 5), into results.json.
set -euo pipefail
scenarios="$1"
adb wait-for-device
adb install -r -t "$RUNNER_TEMP/apps/speedrun-bench.apk"
adb install -r -t "$RUNNER_TEMP/apps/mobile-benchmark.apk"
chmod +x target/release/virtuoso target/release/virtuoso-bench
# The version runs (CLI start) once, in the settings job.
version_runs=0
case "$scenarios" in *launch*) version_runs=10 ;; esac
target/release/virtuoso-bench run --scenarios "$scenarios" --iterations "${2:-5}" --version-runs "$version_runs" --out results.json

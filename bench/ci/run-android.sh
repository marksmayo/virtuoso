#!/bin/bash
# One CI job's share of the Android benchmark, on the emulator the job booted: the benchmark
# apps installed, then every tool (or those of $3, comma-separated) on the scenarios given ($1),
# $2 recorded runs each (default 5), into results.json.
set -euo pipefail
scenarios="$1"
# The competitors were set up in the background while the emulator booted (the workflow's
# step): wait for that setup (up to 30 minutes), show its output, and take its environment.
setup="$RUNNER_TEMP/setup"
if [ -e "$setup.log" ]; then
  for _ in $(seq 1 360); do [ -s "$setup.status" ] && break; sleep 5; done
  cat "$setup.log"
  if [ "$(cat "$setup.status" 2>/dev/null)" != 0 ]; then echo "the competitors' setup failed (or ran past 30 minutes)" >&2; exit 1; fi
  if [ -f "$setup.env" ]; then while IFS='=' read -r k v; do [ -z "$k" ] || export "$k=$v"; done < "$setup.env"; fi
fi
adb wait-for-device
adb install -r -t "$RUNNER_TEMP/apps/speedrun-bench.apk"
adb install -r -t "$RUNNER_TEMP/apps/mobile-benchmark.apk"
chmod +x target/release/virtuoso target/release/virtuoso-bench
# The version runs (CLI start) once, in the settings job.
version_runs=0
case "$scenarios" in *launch*) version_runs=10 ;; esac
tools=()
[ -n "${3:-}" ] && tools=(--tools "$3")
target/release/virtuoso-bench run "${tools[@]}" --scenarios "$scenarios" --iterations "${2:-5}" --version-runs "$version_runs" --out results.json --outputs outputs

#!/bin/bash
# One group of the web benchmark on Chrome on the emulator (web-android-bench.yml), run inside the
# emulator action (which runs each line of its script in a shell of its own, so this is one file).
# Usage: bench/ci/run-web-android.sh <web/flow,...> <iterations> [tools]
set -uo pipefail
adb shell dumpsys package com.android.chrome | grep -m1 versionName || true
export VIRTUOSO_WEB_BIN="$PWD/target/release/virtuoso"
export VIRTUOSO_BENCH_WEB_DEVICE=android
# The runner's memory and the emulator's, every 15 s, to outputs/memory.log: the emulator has
# died mid-run with nothing in its own log.
mkdir -p outputs
(
  while sleep 15; do
    emu=$(ps -C qemu-system-x86_64 -o rss= 2>/dev/null | awk '{s+=$1} END {print int(s/1024)}')
    echo "$(date +%T) free_mb=$(free -m | awk '/^Mem:/ {print $7}') emulator_mb=${emu:-gone} device=$(adb get-state 2>&1 | head -1)"
  done
) > outputs/memory.log 2>&1 &
watch=$!
args=(run --platform web --device emulator-5554 --iterations "${2:-1}" --scenarios "$1" --out results.json --outputs outputs)
[ -n "${3:-}" ] && args+=(--tools "$3")
target/harness/virtuoso-bench "${args[@]}"
status=$?
kill "$watch" 2>/dev/null
echo "--- memory (last lines; all in outputs/memory.log)"; tail -20 outputs/memory.log
echo "--- the kernel on killed processes"; sudo dmesg 2>/dev/null | grep -iE "out of memory|killed process|oom" | tail -10 || true
exit $status

#!/bin/bash
# One group of the web benchmark on Chrome on the emulator (web-android-bench.yml), run inside the
# emulator action (which runs each line of its script in a shell of its own, so this is one file).
# Usage: bench/ci/run-web-android.sh <web/flow,...> <iterations> [tools]
set -euo pipefail
adb shell dumpsys package com.android.chrome | grep -m1 versionName || true
export VIRTUOSO_WEB_BIN="$PWD/target/release/virtuoso"
export VIRTUOSO_BENCH_WEB_DEVICE=android
args=(run --platform web --device emulator-5554 --iterations "${2:-1}" --scenarios "$1" --out results.json --outputs outputs)
[ -n "${3:-}" ] && args+=(--tools "$3")
target/harness/virtuoso-bench "${args[@]}"

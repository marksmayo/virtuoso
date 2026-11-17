#!/bin/bash
# One group of the web benchmark on Chrome on the emulator (web-android-bench.yml), run inside the
# emulator action (which runs each line of its script in a shell of its own, so this is one file).
# Usage: bench/ci/run-web-android.sh <web/flow,...> <iterations> [tools]
#
# The emulator on GitHub's runners crashes now and then (minutes into a run, on its software
# renderer, with memory to spare). So each flow is run a round at a time (every tool once, in
# turn), and a round the emulator did not live through is thrown away whole, the emulator
# started again (EMULATOR_OPTIONS, as the action started it) and the round run again: every
# tool's runs of a round come from the same emulator.
set -uo pipefail
IFS=, read -ra flows <<< "$1"
rounds="${2:-1}"
tools="${3:-}"
export VIRTUOSO_WEB_BIN="$PWD/target/release/virtuoso"
export VIRTUOSO_BENCH_WEB_DEVICE=android
emulator="${ANDROID_SDK_ROOT:-$ANDROID_HOME}/emulator/emulator"
mkdir -p outputs rounds
log() { echo "$(date +%T) $*" | tee -a outputs/emulator.log; }

up() { [ "$(adb get-state 2>/dev/null)" = device ] && [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; }

restart() {
  log "emulator gone: starting it again"
  adb emu kill > /dev/null 2>&1
  pkill -f qemu-system > /dev/null 2>&1
  sleep 3
  # shellcheck disable=SC2086
  nohup "$emulator" -port 5554 -avd test ${EMULATOR_OPTIONS:-} >> outputs/emulator-restarts.log 2>&1 &
  adb wait-for-device
  for _ in $(seq 120); do up && break; sleep 5; done
  up || { log "the emulator did not come back"; return 1; }
  adb shell input keyevent 82 > /dev/null 2>&1
  log "emulator back"
}

adb shell dumpsys package com.android.chrome | grep -m1 versionName || true
# The runner's memory and the emulator's, every 15 s.
(
  while sleep 15; do
    emu=$(ps -C qemu-system-x86_64 -o rss= 2>/dev/null | awk '{s+=$1} END {print int(s/1024)}')
    echo "$(date +%T) free_mb=$(free -m | awk '/^Mem:/ {print $7}') emulator_mb=${emu:-gone} device=$(adb get-state 2>&1 | head -1)"
  done
) > outputs/memory.log 2>&1 &
watch=$!

lost=0
for flow in "${flows[@]}"; do
  for round in $(seq "$rounds"); do
    name="$(echo "$flow" | tr '/' '_')-$round"
    for attempt in 1 2 3; do
      up || restart || break
      args=(run --platform web --device emulator-5554 --iterations 1 --scenarios "$flow" --out "rounds/$name.json" --outputs "outputs/$name")
      [ -n "$tools" ] && args+=(--tools "$tools")
      target/harness/virtuoso-bench "${args[@]}"
      if up; then break; fi
      lost=$((lost + 1))
      log "$flow round $round: the emulator crashed during it (attempt $attempt)"
      # The last attempt's results are kept, crash and all: its failures say so.
      [ "$attempt" -lt 3 ] && rm -f "rounds/$name.json"
    done
  done
done
kill "$watch" 2>/dev/null

files=(rounds/*.json)
[ -e "${files[0]}" ] || { echo "no rounds recorded"; exit 1; }
target/harness/virtuoso-bench merge results.json "${files[@]}"
# Each round was recorded as iteration 0: number them per tool and flow.
jq '.runs |= (group_by([.tool, .scenario]) | map(to_entries | map(.value + {iteration: .key})) | flatten)' results.json > results.tmp && mv results.tmp results.json
echo "--- rounds lost to an emulator crash and run again: $lost"
echo "--- memory (last lines; all in outputs/memory.log)"; tail -10 outputs/memory.log

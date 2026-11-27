#!/bin/bash
# One group of the web benchmark on Chrome on the emulator (web-android-bench.yml), run inside the
# emulator action (which runs each line of its script in a shell of its own, so this is one file).
# Usage: bench/ci/run-web-android.sh <web/flow,...> <iterations> [tools]
#
# The emulator on GitHub's runners crashes now and then (minutes into a run, on its software
# renderer, with memory to spare). So each flow is run a round at a time (every tool once, in
# turn), and a round the emulator did not live through is thrown away whole, the emulator
# started again (EMULATOR_OPTIONS, as the action started it) and the round run again (up to five
# times; then the round is left out): every tool's runs of a round come from the same emulator.
set -uo pipefail
IFS=, read -ra flows <<< "$1"
rounds="${2:-1}"
tools="${3:-}"
export VIRTUOSO_WEB_BIN="$PWD/target/release/virtuoso"
export VIRTUOSO_BENCH_WEB_DEVICE=android
emulator="${ANDROID_SDK_ROOT:-$ANDROID_HOME}/emulator/emulator"
mkdir -p outputs rounds
log() { echo "$(date +%T) $*" | tee -a outputs/emulator.log; }

# No on-screen keyboard, as in the desktop and Pixel 7 fields: it shortens and moves the visible
# part of the page after each typed field, and every tool's next tap then lands on what had been
# above its target (onboarding_wizard's Next, for all of them). The tools type through DevTools or
# adb, not the keyboard app.
no_keyboard() {
  for ime in $(adb shell ime list -s 2>/dev/null | tr -d '\r'); do
    # Appium's own input methods show no keyboard, and their app is the one Appium needs.
    case "$ime" in io.appium.*) continue ;; esac
    adb shell pm disable-user --user 0 "${ime%%/*}" > /dev/null 2>&1
  done
  # Nor Android's "isn't responding" dialogs: on a slow emulator System UI is reported as not
  # responding, and its dialog covers the page for the tools that read the screen (Maestro, through
  # the accessibility tree) and not for those that read it through DevTools.
  adb shell settings put global hide_error_dialogs 1 > /dev/null 2>&1
}

browser="${VIRTUOSO_ANDROID_BROWSER:-com.android.chrome}"
# A Chromium build to drive instead of the image's Chrome (CHROMIUM_APK, from
# bench/ci/install-android-browser.sh download; VIRTUOSO_ANDROID_BROWSER=org.chromium.chrome):
# installed now, and again should a restarted emulator have lost it.
chromium() {
  [ -n "${CHROMIUM_APK:-}" ] || return 0
  adb shell pm path "$browser" 2>/dev/null | grep -q package: && return 0
  bash bench/ci/install-android-browser.sh install "$CHROMIUM_APK"
}

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
  # A crash can leave a tool's helper app half installed (Appium's settings app then has no
  # activity): each tool installs its own again.
  for app in io.appium.settings io.appium.uiautomator2.server io.appium.uiautomator2.server.test dev.mobile.maestro dev.mobile.maestro.test; do
    adb uninstall "$app" > /dev/null 2>&1
  done
  no_keyboard
  chromium
  log "emulator back"
}

chromium || { echo "the Chromium build did not install"; exit 1; }

# The browser started once before any round: a browser that cannot start (Chromium 159 with the
# emulator's Vulkan off aborted at every start) fails every tool's every run, and Playwright waits
# 900 s for each. No DevTools socket within 90 s and the job ends here, saying so.
browser_starts() {
  adb shell am start -W -a android.intent.action.VIEW -d about:blank -p "$browser" > /dev/null 2>&1
  for _ in $(seq 45); do
    adb shell cat /proc/net/unix 2>/dev/null | grep -q devtools_remote && { adb shell am force-stop "$browser"; return 0; }
    sleep 2
  done
  adb shell am force-stop "$browser" > /dev/null 2>&1
  return 1
}
browser_starts || { echo "$browser did not start: no DevTools socket within 90 s (logcat in outputs/browser-start.log)"; mkdir -p outputs; adb logcat -d > outputs/browser-start.log 2>&1; exit 1; }
echo "$browser:$(adb shell dumpsys package "$browser" | grep -m1 versionName | tr -d ' \r')"
no_keyboard
echo "input methods left: $(adb shell ime list -s 2>/dev/null | tr -d '\r' | tr '\n' ' ')"
# Asked to diagnose: when a page on this browser is ready for input after its load, five times.
[ -n "${BENCH_DIAGNOSE:-}" ] && node bench/ci/web-android-probe.mjs emulator-5554 "$browser" 5 2>&1 | tee outputs/probe.log
# The runner's memory and the emulator's, every 15 s. With BENCH_DIAGNOSE set (the workflow's
# `diagnose` input), also the window in focus and the device's own screen: a DevTools screenshot
# shows only the page, and a native window over the browser (an ANR dialog, a first-run screen)
# is seen only there. Off for a field's run: each costs the emulator a little.
mkdir -p outputs/screens
(
  while sleep 15; do
    emu=$(ps -C qemu-system-x86_64 -o rss= 2>/dev/null | awk '{s+=$1} END {print int(s/1024)}')
    line="$(date +%T) free_mb=$(free -m | awk '/^Mem:/ {print $7}') emulator_mb=${emu:-gone} device=$(adb get-state 2>&1 | head -1)"
    if [ -n "${BENCH_DIAGNOSE:-}" ]; then
      line="$line focus=$(adb shell dumpsys window 2>/dev/null | grep -m1 mCurrentFocus | tr -d '\r' | sed 's/^ *mCurrentFocus=//')"
      adb exec-out screencap -p > outputs/screens/"$(date +%H%M%S)".png 2>/dev/null
    fi
    echo "$line"
  done
) > outputs/memory.log 2>&1 &
watch=$!

lost=0
for flow in "${flows[@]}"; do
  for round in $(seq "$rounds"); do
    name="$(echo "$flow" | tr '/' '_')-$round"
    for attempt in 1 2 3 4 5; do
      up || restart || break
      args=(run --platform web --device emulator-5554 --iterations 1 --scenarios "$flow" --out "rounds/$name.json" --outputs "outputs/$name")
      [ -n "$tools" ] && args+=(--tools "$tools")
      target/harness/virtuoso-bench "${args[@]}"
      if up; then break; fi
      lost=$((lost + 1))
      log "$flow round $round: the emulator crashed during it (attempt $attempt)"
      # A round the emulator never lived through says nothing of the tools: none is kept.
      rm -f "rounds/$name.json"
    done
  done
done
kill "$watch" 2>/dev/null
# The device's ANRs and crashes (an app or System UI not responding puts a dialog over the page).
adb logcat -d -b events 2>/dev/null | grep -E "am_anr|am_crash|am_proc_died" > outputs/anr.log || true
# virtuoso's own record of each run (screenshots and screen text at a failure).
cp -r "$HOME/.virtuoso/tests" outputs/virtuoso-debug 2>/dev/null || true

files=(rounds/*.json)
[ -e "${files[0]}" ] || { echo "no rounds recorded"; exit 1; }
target/harness/virtuoso-bench merge results.json "${files[@]}"
# Each round was recorded as iteration 0 of a batch of its own: number them per tool and flow,
# and put them in the job's first batch, so the rounds are scored together as one batch of runs
# (separate batches would score virtuoso's last round and each other tool's best).
jq '(.runs | map(.batch) | min) as $b | .runs |= (group_by([.tool, .scenario]) | map(to_entries | map(.value + {iteration: .key, batch: $b})) | flatten)' results.json > results.tmp && mv results.tmp results.json
echo "--- rounds lost to an emulator crash and run again: $lost"
echo "--- memory (last lines; all in outputs/memory.log)"; tail -10 outputs/memory.log

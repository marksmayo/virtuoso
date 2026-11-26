#!/bin/bash
# A Chromium build for the web benchmark's Android emulator field, from Chromium's own continuous
# builds (commondatastorage.googleapis.com/chromium-browser-snapshots, the only source used): the
# x86_64 build (AndroidDesktop_x64; the Android and Android_Arm64 builds are ARM) at a main-branch
# position, its ChromePublic.apk (package org.chromium.chrome, Chrome's main activity, DevTools on
# chrome_devtools_remote). A Chrome milestone's position is its branch point
# (chromiumdash.appspot.com/fetch_milestones; 158 is 1727472): the build at or just below it is
# that milestone's first (158 -> 1727419, 158.0.8127.0; 159, branched at 1739869 -> 1739839,
# 159.0.8154.0).
#
#   install-android-browser.sh download <position> <dir>   the APK to <dir>/ChromePublic.apk
#   install-android-browser.sh install <apk>               on the device adb names (ANDROID_SERIAL)
#
# Installed, it is made the device's browser (Maestro opens a link in the default browser), its
# notification prompt answered (it covers the page on a first start) and the image's Chrome
# disabled (see below); the tools then drive it
# with VIRTUOSO_ANDROID_BROWSER=org.chromium.chrome.
set -euo pipefail
bucket=https://commondatastorage.googleapis.com/chromium-browser-snapshots/AndroidDesktop_x64
pkg=org.chromium.chrome

case "${1:-}" in
  download)
    pos="$2"; dir="$3"
    [[ "$pos" =~ ^[0-9]+$ ]] || { echo "a position is a number, not \`$pos\`" >&2; exit 2; }
    mkdir -p "$dir"
    if [ ! -f "$dir/ChromePublic.apk" ]; then
      # The build's own record of what it was built from: the position asked for.
      built=$(curl -fsSL --retry 5 "$bucket/$pos/REVISIONS" | sed -n 's/.*"chromium_revision": *"\([0-9]*\)".*/\1/p' || true)
      [ "$built" = "$pos" ] || { echo "no x86_64 Chromium build at $pos (its REVISIONS say \`$built\`)" >&2; exit 1; }
      curl -fsSL --retry 5 -o "$dir/chromium.zip" "$bucket/$pos/chrome-android-desktop.zip"
      unzip -q -o -j "$dir/chromium.zip" chrome-android-desktop/apks/ChromePublic.apk -d "$dir"
      rm -f "$dir/chromium.zip"
    fi
    ls -l "$dir/ChromePublic.apk"
    ;;
  install)
    apk="$2"
    # A build of another position (a higher version code) refuses to be replaced by an older one.
    adb uninstall "$pkg" > /dev/null 2>&1 || true
    adb install "$apk"
    adb shell pm grant "$pkg" android.permission.POST_NOTIFICATIONS
    adb shell cmd role add-role-holder android.app.role.BROWSER "$pkg"
    # The image's Chrome off: running, it holds the DevTools socket (chrome_devtools_remote) and
    # Chromium's goes to chrome_devtools_remote_<pid>, which no tool looks for.
    adb shell am force-stop com.android.chrome
    adb shell pm disable-user --user 0 com.android.chrome
    adb shell dumpsys package "$pkg" | grep -m1 versionName
    ;;
  *)
    sed -n '2,15p' "$0" >&2
    exit 2
    ;;
esac

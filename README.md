# virtuoso

[![CI](https://github.com/marksmayo/virtuoso/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/marksmayo/virtuoso/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-Android%20%7C%20iOS%20%7C%20Web-lightgrey.svg)](#benchmark)
[![Maestro compatible](https://img.shields.io/badge/flows-Maestro%20compatible-6f42c1.svg)](https://github.com/mobile-dev-inc/maestro)
[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://app.codspeed.io/marksmayo/virtuoso?utm_source=badge)

A 2.2 MB, Maestro-compatible end-to-end test runner for Android, iOS, and Chrome, written in Rust.
One binary with its device agents built in—waiting for the app to settle instead of sleeping on the
clock, and reading the accessibility/DOM tree without screenshots.

### At a glance (Android on Linux CI, median of 5 runs)

| Metric | Maestro (JVM) | maestro-runner (Go) | speedrun | **virtuoso (Rust)** |
|---|---:|---:|---:|---:|
| **Install size** | 361.5 MB | 49.5 MB | 3.7 MB | **2.2 MB** |
| **One-command flow (connect + run)** | 13.54 s | 3.47 s | 1.32 s | **0.47 s** |
| **Core suite (sum of medians)** | 198.01 s | 100.00 s | 43.10 s | **38.80 s** |
| **Peak host memory** | 654.6 MB | 26.9 MB | 5.8 MB | **4.6 MB** |
| **App flows passed** | 60% | 40% | 95% | **100%** |

*(Full methodology, Windows/macOS runners, and 186 individual scenario metrics in [Benchmark](#benchmark).)*

```yaml
appId: com.example.app
---
- launchApp
- tapOn: "Sign in"
- inputText: "user@example.com"
- tapOn: "Continue"
- assertVisible: "Welcome"
```

```sh
virtuoso test flows/                                   # a folder, as `maestro test` takes it
virtuoso test login.yaml -e PASSWORD=secret --format junit --output report.xml
virtuoso --device emulator-5554,emulator-5556 test flows/ --shard-split 2
virtuoso lint flows/                                   # check flows without a device
virtuoso hierarchy                                     # what is on screen now
virtuoso test flows/ --app-file app.apk --video --format allure
virtuoso test flows/ --continuous                      # run again on every change
virtuoso --auto-start-emulator test flows/             # boot an AVD when nothing is connected
virtuoso --platform web test web-flows/                # a headless Chrome (url: in the flow)
virtuoso --platform web --emulate pixel-7 test web-flows/  # the same, as a phone (size, touch, UA)
virtuoso --platform web --device android test web-flows/  # Chrome on an Android device, over adb
virtuoso session start --app com.example.app           # drive an app a command at a time:
virtuoso do launchApp 'tapOn: Sign in' && virtuoso look  #   run commands, read the screen
virtuoso session save flows/sign-in.yaml               #   the commands that passed, as a flow
virtuoso doctor | virtuoso latency | virtuoso skills       # toolchain check, agent round trips, AI skill
```

## What it supports

The Maestro flow format: every command a Maestro Android flow uses (taps, text, swipes,
scrolling, assertions, app control, links, keys, clipboard, screenshots and visual assertions,
recordings, location and travel, orientation, permissions, media, airplane and dark mode,
`runFlow` (with maestro-runner's `else`), `repeat`, `retry`, scripts) plus maestro-runner's
`wait`, `runShell`, `dragAndDrop`, `openNotifications`, `acceptAlert` and `dismissAlert`; every
selector (text and id regexes with the full syntax, index, states, relative positions,
hierarchy, size, traits), `${...}` JavaScript expressions on QuickJS with `http` (multipart
forms too), `faker` and `require()`, `config.yaml` with globs, tags and execution order, hooks,
JUnit / HTML / JSON / Allure reports, videos, variable values masked in reports, sharding across
devices, and a restart of the device agent if it dies mid-run. `bench/features.json` lists every feature, against maestro-runner and
speedrun, with the reason behind each partial or missing one.

Android devices and emulators, iOS simulators (`--platform ios`, on a Mac with Xcode; not real
iPhones yet), and web pages in Chrome (`--platform web`, headless unless `--headed`). Runs on Windows (natively), Linux and macOS. On iOS, what a simulator does not
have is not supported, as in Maestro: airplane mode, the Back key, the volume keys; the camera and
notification permissions are the app's to ask for (simctl cannot set them).

On the web a flow names its page with `url:` where a phone flow has `appId:`, and selectors match
as in Maestro's web mode: `id` is the element's `id`, else its `data-testid`; text is the
element's own text (text inside an inline tag is that tag's), a field's value or its placeholder.
`launchApp` opens the page in a fresh tab (`clearState` first forgets the site's cookies and
storage), a swipe is the mouse wheel, `dragAndDrop` a mouse drag (HTML5 drag and drop too), and
the browser's dialogs, the tabs a page opens, frames (from another origin too) and open shadow
roots are on the screen like the rest. `setLocation`, `setPermissions`, dark mode, airplane mode
(offline), `setOrientation` (the viewport turned) and screenshots work; a recording needs ffmpeg
on the PATH. `--device` is `chrome`, a browser's path, or `ws://...` to drive one already
running. What a page has no use for (the volume keys, notifications, media) is not supported.

## How it works

- `agent/android` — a small Java agent (no Gradle: `javac` + `d8`, built by `build.rs` and
  embedded in the binary). It runs as the shell user through `app_process`, reads every
  window's accessibility tree through a hidden `UiAutomation`, injects input straight into the
  input manager, runs device commands without an adb round trip, and tells when the UI has
  settled (accessibility events going quiet, a main-thread idle probe, and a stable-tree
  check for screens that never stop animating). It speaks length-prefixed JSON over one TCP
  connection through `adb forward`, after a per-run token.
- `agent/ios` — the iOS agent: an XCUITest bundle (Swift) that the binary carries as source and
  builds for the simulator on first use. It serves the same protocol as the Android agent on
  the simulator's (shared) network, sends the screen in the same node format, and answers taps,
  drags, keys and text through XCUITest; the host does the rest with `xcrun simctl` (links,
  location, permissions, media, dark mode, video, clipboard, keychain).
- The web has no agent: virtuoso drives Chrome over its DevTools Protocol, on a WebSocket client
  of its own (`websocket`, `chrome`). The screen is one script run in the page (`web_screen.js`);
  a settle waits on a second, installed before each page's own scripts (`web_tracker.js`), that
  knows the last DOM change, the requests in flight, the short timers and the animations running.
- `crates/virtuoso` — the host: flow parser (`flow`), selector matching (`select`), the runner
  (`runner`, against a `Device` trait), the Android device (`android`, `android_device`), the
  iOS simulator (`ios`), the web (`web`),
  JavaScript (`js`), suites (`workspace`), reports (`report`) and the CLI (`main`).
- `crates/virtuoso-bench` — the benchmark harness and the HTML medal report.

## Building

You need Rust, a JDK (17+) and the Android SDK (`ANDROID_HOME`, with a platform and
build-tools). Then:

```sh
cargo build --release          # target/release/virtuoso, agent included
```

Without a JDK or SDK the binary builds with no agent and refuses Android devices.
`VIRTUOSO_AGENT_JAR=<path>` embeds an agent built elsewhere (cross builds).

For iOS, nothing more at build time: on a Mac with Xcode, the first `--platform ios` run
builds the iOS agent for the simulator (about a minute) into `~/Library/Caches/virtuoso` and
keeps it until the agent changes. `VIRTUOSO_IOS_AGENT=<path>` uses one built elsewhere (its
`.xctestrun`). The agent's Xcode project is generated from `agent/ios/project.yml` with
XcodeGen and committed; CI checks the two match.

## Tests

Every feature was built test first (red, then green). Most tests need no device: the runner
runs against a scripted `FakeDevice` (`crates/virtuoso/src/testing.rs`), the parser and
selectors against literals. Tests that need a real device or emulator pass trivially unless
`VIRTUOSO_DEVICE` names one:

```sh
cargo test --workspace                                            # no device needed
VIRTUOSO_DEVICE=emulator-5554 cargo test -p virtuoso -- --test-threads=1   # also on a device
```

The iOS tests need a booted simulator with the fixture app (`agent/ios/Fixture`) installed:
`VIRTUOSO_IOS_DEVICE=<udid> cargo test -p virtuoso --test device_ios --test ios_flows --
--test-threads=1`. GitHub Actions runs them on a macOS runner for every push
(`.github/workflows/ios.yml`): it boots a simulator, builds the fixture app, and lets virtuoso
build its agent as it would on any Mac.

`VIRTUOSO_DEBUG=1` traces every agent call with its time.

The host-side hot paths (flow parsing, screen replies, selectors, `${...}` expansion, PNG
decode/compare) have micro-benchmarks in `crates/virtuoso-benches/benches/core.rs`, tracked on every
push and pull request by [CodSpeed](https://app.codspeed.io/marksmayo/virtuoso)
(`.github/workflows/codspeed.yml`):

```sh
cargo bench -p virtuoso --bench core
```

## Benchmark

`virtuoso-bench` runs the same flows with each tool — virtuoso, maestro-runner, speedrun,
Maestro, Appium, Tester Army's e2e and Software Mansion's Argent — resets the device before
every run, and writes a results file; `virtuoso-bench report` turns it into a self-contained
HTML page with the medal tables. Three sets of flows: `bench/flows` against Android's Settings
app, speedrun's own benchmark app, and Tester Army's mobile benchmark app (`bench/tam-flows`).
See [docs/benchmark.md](docs/benchmark.md) for the metrics and how to run it.

```sh
cargo build --release -p virtuoso -p virtuoso-bench
target/release/virtuoso-bench run --iterations 5 --out bench/results/results.json
target/release/virtuoso-bench report bench/results/results.json --out report/index.html
```

The results below (between the `benchmark` markers) come from GitHub Actions runs, one field per
platform and host, every tool measured together on the same device: `virtuoso-bench readme
--field <name>=<results.json>... --charts docs/bench` writes them and their charts. A metric is
a medal event of its own in each field, so a tool that wins on one platform and loses on another
shows it.

### Latest results

<!-- benchmark:start -->
Each of the 185 performance events and 55 features is a medal event. A performance metric is an event of its own on each platform and host (Android (Linux runner), Android (Windows runner), iOS (macOS runner)), so a tool that wins on one and not another shows it. Written by `virtuoso-bench readme`, from GitHub Actions runs.

**Overall**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 197 | 14 | 2 | 213 |
| [speedrun](https://github.com/hubastard/speedrun) | 45 | 79 | 6 | 130 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 36 | 29 | 65 | 130 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 28 | 3 | 7 | 38 |
| [argent](https://github.com/software-mansion/argent) | 7 | 29 | 13 | 49 |
| [e2e](https://github.com/tester-army/e2e) | 4 | 12 | 49 | 65 |
| [appium](https://github.com/appium/appium) | 1 | 1 | 17 | 19 |

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 152 | 14 | 2 | 168 |
| [speedrun](https://github.com/hubastard/speedrun) | 23 | 78 | 4 | 105 |
| [argent](https://github.com/software-mansion/argent) | 7 | 29 | 13 | 49 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 5 | 0 | 3 | 8 |
| [e2e](https://github.com/tester-army/e2e) | 4 | 12 | 49 | 65 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 28 | 62 | 91 |
| [appium](https://github.com/appium/appium) | 1 | 1 | 17 | 19 |

### Android (Linux runner)

![Android (Linux runner)](docs/bench/android-linux-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub ubuntu-latest runner (4 vCPU, 16 GB), Android emulator on KVM.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 59 | 1 | 0 | 60 |
| [speedrun](https://github.com/hubastard/speedrun) | 5 | 51 | 1 | 57 |
| [argent](https://github.com/software-mansion/argent) | 3 | 1 | 5 | 9 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 3 | 0 | 0 | 3 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 2 | 24 | 28 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 1 | 24 | 26 |
| [appium](https://github.com/appium/appium) | 1 | 0 | 5 | 6 |

![Android (Linux runner): every scenario](docs/bench/android-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 49.5 MB | 3.7 MB | **2.4 MB** | 396.3 MB | 147.7 MB | 42.8 MB | 243.9 MB |
| CLI start (--version) | 13 ms | 3 ms | **3 ms** | 2953 ms | 736 ms | 43 ms | 49 ms |
| Invalid flow reported | 13 ms | 2 ms | **1 ms** | 3292 ms | — | — | 1270 ms |
| One-command flow (connect + run) | 2.61 s | 1.18 s | **0.35 s** | 16.44 s | 12.43 s | 9.06 s | 2.20 s |
| App launch | 7.64 s | 2.32 s | **1.93 s** | 19.16 s | 11.45 s | 13.12 s | 6.39 s |
| Navigation | 19.38 s | 6.41 s | **6.17 s** | 39.57 s | 26.68 s | 27.38 s | 23.05 s |
| Scroll to element | 18.88 s | 4.75 s | **4.00 s** | 30.03 s | 28.80 s | 20.28 s | 17.00 s |
| Text input | 15.77 s | 8.20 s | **6.21 s** | 47.67 s | 35.88 s | 21.07 s | 18.06 s |
| Assertion burst | 7.40 s | 2.78 s | **1.89 s** | 21.62 s | 12.46 s | 13.85 s | 7.92 s |
| Repeated taps | 26.65 s | 9.04 s | **8.76 s** | 51.57 s | 33.58 s | 35.64 s | 32.13 s |
| Whole suite (sum of medians) | 95.71 s | 33.51 s | **28.97 s** | 209.62 s | 148.85 s | 131.34 s | 104.56 s |
| Peak host memory | 26.6 MB | 5.4 MB | **2.1 MB** | 660.4 MB | 179.1 MB | 154.6 MB | 131.9 MB |
| Host CPU time, whole suite | 2.35 s | 1.18 s | **0.04 s** | 156.42 s | 16.44 s | 25.62 s | 10.69 s |
| Peak device agent memory | 131.4 MB | 147.9 MB | 133.3 MB | 158.5 MB | 284.8 MB | — | **89.7 MB** |
| Runs passed | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** |
| Run-to-run variation | 6.7% (SD 972 ms) | 4.3% (SD 265 ms) | 4.8% (SD 188 ms) | **1.9%** (SD 621 ms) | 3.8% (SD 929 ms) | 2.4% (SD 595 ms) | 4.1% (SD 670 ms) |
| app/animations | 134.16 s | 62.40 s | **31.35 s** | 787.96 s | 67.97 s | 68.64 s | 180.82 s |
| app/appearance | 28.64 s | 8.70 s | **7.15 s** | 151.94 s | 40.52 s | 26.44 s | 41.84 s |
| app/cart | 61.16 s | 21.76 s | **16.26 s** | — | 64.76 s | 48.62 s | — |
| app/chat | 34.66 s | 20.31 s | **17.70 s** | 172.02 s | 45.75 s | 33.53 s | 54.14 s |
| app/deep_links | 41.53 s | 27.39 s | 26.95 s | 112.33 s | 24.81 s | 22.25 s | **20.78 s** |
| app/device | — | 31.45 s | **27.85 s** | 277.59 s | 76.64 s | — | — |
| app/drag | — | 19.08 s | **15.48 s** | — | 93.57 s | 65.71 s | 126.90 s |
| app/feed | — | 33.78 s | **21.44 s** | — | 254.79 s | 100.43 s | 357.58 s |
| app/gestures | 25.02 s | 9.62 s | **7.86 s** | 149.41 s | 36.77 s | 25.00 s | 45.34 s |
| app/inputs | — | — | **36.92 s** | — | — | — | — |
| app/native | — | 18.11 s | **17.67 s** | — | — | 46.96 s | — |
| app/onboarding | — | 8.08 s | **6.67 s** | — | 34.17 s | 40.77 s | 71.71 s |
| app/popups | — | 32.06 s | **29.79 s** | 187.34 s | 72.30 s | 56.68 s | 63.71 s |
| app/profile | — | 28.57 s | **27.19 s** | 363.81 s | 114.85 s | 57.20 s | 126.76 s |
| app/remember_me | — | 18.39 s | **16.83 s** | 270.48 s | 119.53 s | 43.23 s | 105.88 s |
| app/scripting | — | 14.43 s | **12.67 s** | 139.48 s | 33.33 s | 24.87 s | 39.45 s |
| app/selectors | — | 24.64 s | **16.53 s** | 197.46 s | 49.11 s | 35.17 s | 53.37 s |
| app/shop_checkout | — | 73.21 s | **67.37 s** | — | — | — | — |
| app/sign_in | 35.68 s | 11.19 s | **10.30 s** | 201.46 s | 50.45 s | 29.93 s | 60.55 s |
| app/timing | 49.06 s | 17.22 s | **15.96 s** | 209.61 s | 60.88 s | 39.59 s | 73.85 s |
| app/Whole app suite (sum) | — | — | **429.96 s** | — | — | — | — |
| app/App flows passed | 40% | 94% | **99%** | 65% | 83% | 86% | 74% |
| tam/async_states | 19.92 s | 16.84 s | **15.69 s** | 34.91 s | 19.47 s | 18.33 s | 16.19 s |
| tam/bottom_tabs | 9.18 s | 3.57 s | **3.03 s** | 27.70 s | 14.30 s | 14.66 s | 9.59 s |
| tam/broken_accessibility | 9.34 s | 4.16 s | **3.67 s** | 28.76 s | 15.50 s | 14.57 s | 13.47 s |
| tam/choice_controls | 10.46 s | 3.50 s | **3.26 s** | 29.41 s | 14.22 s | 12.95 s | 11.81 s |
| tam/control_inventory | 18.98 s | 9.84 s | **8.37 s** | 52.86 s | — | 16.55 s | 22.44 s |
| tam/debounced_search | 8.71 s | **4.74 s** | **4.71 s** | 28.00 s | 15.47 s | 12.78 s | 14.05 s |
| tam/error_recovery | 12.85 s | 6.60 s | **5.76 s** | 32.58 s | 17.81 s | 18.15 s | 14.11 s |
| tam/flattened_login | 10.40 s | 7.18 s | **6.27 s** | 36.66 s | 18.66 s | 15.21 s | 18.07 s |
| tam/flattened_registration_form | 10.92 s | **7.52 s** | **7.51 s** | 39.99 s | 19.65 s | 13.35 s | 21.82 s |
| tam/gestures | 8.18 s | 4.85 s | **4.03 s** | 27.20 s | 14.30 s | 13.75 s | — |
| tam/huge_virtualized_list | — | 16.91 s | **10.57 s** | 184.53 s | 146.03 s | 68.78 s | 133.89 s |
| tam/infinite_scroll_list | 43.34 s | 7.65 s | **6.51 s** | 56.65 s | 39.44 s | 22.29 s | 36.43 s |
| tam/login_form | 10.06 s | 6.09 s | **5.07 s** | 37.22 s | 18.11 s | 12.83 s | 17.00 s |
| tam/modal_flow | 9.87 s | 4.21 s | **3.75 s** | 26.98 s | 14.66 s | 14.51 s | — |
| tam/permission_prompt | 13.60 s | 10.03 s | **9.73 s** | 26.74 s | 13.22 s | 12.67 s | — |
| tam/photo_picker | 11.76 s | 5.71 s | **5.10 s** | 29.75 s | 17.46 s | 17.41 s | 13.41 s |
| tam/product_catalog | 8.36 s | 3.61 s | **2.88 s** | 27.92 s | 14.11 s | 13.75 s | 10.81 s |
| tam/sequential_onboarding | 15.27 s | 9.00 s | **7.33 s** | 51.67 s | 30.08 s | 14.33 s | 25.38 s |
| tam/sticky_chrome_target | 12.96 s | 4.19 s | **3.08 s** | 31.09 s | 18.11 s | 14.83 s | 14.03 s |
| tam/text_input_variations | 10.87 s | **6.79 s** | **6.85 s** | 38.82 s | 17.71 s | 12.83 s | 18.50 s |
| tam/vision_only | 8.85 s | 3.01 s | **2.55 s** | 21.42 s | 11.98 s | 11.45 s | 7.80 s |
| tam/webview_accessibility | 17.68 s | 7.41 s | **6.37 s** | 36.02 s | 19.92 s | 16.39 s | 16.14 s |
| tam/Whole app suite (sum) | — | 153.42 s | **132.10 s** | 906.91 s | — | 382.38 s | — |
| tam/App flows passed | 95% | **100%** | **100%** | **99%** | 95% | **100%** | 84% |

</details>

### Android (Windows runner)

![Android (Windows runner)](docs/bench/android-windows-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub windows-latest runner (4 vCPU, 16 GB), Android emulator on WHPX.

Not measured: speedrun, which runs on Linux and macOS only, so here in WSL: under WSL 1 it finds no flows, and WSL 2 cannot reach the emulator's adb-forwarded ports on GitHub's Windows runners (mirrored networking is unsupported there).

virtuoso here is its native Windows build, as every other tool here runs natively; its Linux build, run in WSL, is not scored.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 58 | 0 | 2 | 60 |
| [argent](https://github.com/software-mansion/argent) | 4 | 28 | 8 | 40 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 2 | 0 | 0 | 2 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 0 | 20 | 18 | 38 |
| [e2e](https://github.com/tester-army/e2e) | 0 | 7 | 8 | 15 |
| [appium](https://github.com/appium/appium) | 0 | 1 | 12 | 13 |

![Android (Windows runner): every scenario](docs/bench/android-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|
| Install size | 59.0 MB | **2.4 MB** | 361.5 MB | 129.8 MB | 41.4 MB | 238.8 MB |
| CLI start (--version) | 22 ms | **8 ms** | 3355 ms | 1352 ms | 82 ms | 93 ms |
| Invalid flow reported | 89 ms | **10 ms** | 4858 ms | — | — | 620 ms |
| One-command flow (connect + run) | 11.74 s | **1.89 s** | 37.38 s | 33.31 s | 25.49 s | 6.19 s |
| App launch | 24.64 s | **7.51 s** | 40.74 s | 27.84 s | 32.75 s | 16.52 s |
| Navigation | 60.17 s | **27.89 s** | 87.41 s | 65.81 s | 78.05 s | 52.09 s |
| Scroll to element | 50.09 s | **15.81 s** | 67.25 s | 63.25 s | 60.31 s | 36.08 s |
| Text input | 42.21 s | **21.34 s** | 114.40 s | 83.66 s | — | 42.74 s |
| Assertion burst | 30.06 s | **8.17 s** | 46.63 s | 32.68 s | 43.22 s | 22.15 s |
| Repeated taps | — | **39.81 s** | 114.71 s | 91.55 s | 103.06 s | 68.89 s |
| Whole suite (sum of medians) | — | **120.54 s** | 471.15 s | 364.79 s | — | 238.47 s |
| Peak host memory | 30.0 MB | **5.9 MB** | 588.8 MB | 145.7 MB | 35.2 MB | 73.2 MB |
| Host CPU time, whole suite | 17.03 s | **0.08 s** | 215.84 s | 37.14 s | 106.47 s | 2.77 s |
| Peak device agent memory | 120.6 MB | 133.2 MB | 157.3 MB | 289.0 MB | — | **88.5 MB** |
| Runs passed | 83% | **100%** | **100%** | 87% | 77% | **100%** |
| Run-to-run variation | 12.2% (SD 4979 ms) | 7.8% (SD 1177 ms) | 9.0% (SD 4551 ms) | 10.3% (SD 3586 ms) | 5.2% (SD 2323 ms) | **4.9%** (SD 1280 ms) |
| app/animations | 242.06 s | **61.67 s** | — | 166.78 s | 207.58 s | 224.51 s |
| app/appearance | 88.61 s | **41.98 s** | 431.70 s | 137.34 s | 105.79 s | — |
| app/cart | — | **40.24 s** | — | — | — | — |
| app/chat | 67.21 s | **32.60 s** | — | 98.94 s | 79.77 s | 76.31 s |
| app/deep_links | 77.86 s | **37.01 s** | 227.54 s | 67.92 s | — | **37.30 s** |
| app/device | — | — | — | — | — | — |
| app/drag | — | **65.09 s** | — | 258.02 s | 212.15 s | 273.47 s |
| app/feed | — | **51.32 s** | — | — | — | — |
| app/gestures | 47.87 s | **13.67 s** | 244.31 s | 71.38 s | 64.64 s | 51.93 s |
| app/inputs | — | **113.93 s** | — | — | — | — |
| app/native | — | **39.57 s** | — | — | — | — |
| app/onboarding | — | **21.34 s** | — | 100.93 s | 123.94 s | 98.20 s |
| app/popups | — | **34.08 s** | 235.91 s | 88.94 s | 82.09 s | 61.45 s |
| app/profile | — | **75.19 s** | — | 300.94 s | 213.08 s | — |
| app/remember_me | — | **41.33 s** | 374.57 s | 239.74 s | 115.29 s | 137.97 s |
| app/scripting | — | **18.95 s** | 264.33 s | 71.67 s | 57.84 s | 45.94 s |
| app/selectors | — | **27.06 s** | 283.69 s | — | 97.38 s | 69.15 s |
| app/shop_checkout | — | **108.13 s** | — | — | — | — |
| app/sign_in | 105.71 s | **31.17 s** | 412.86 s | 156.17 s | — | — |
| app/timing | 96.18 s | **31.77 s** | 358.01 s | 127.56 s | 169.31 s | 91.62 s |
| app/Whole app suite (sum) | — | — | — | — | — | — |
| app/App flows passed | 34% | **92%** | 46% | 61% | 63% | 59% |
| tam/async_states | 27.81 s | **18.51 s** | 43.70 s | 28.47 s | 30.15 s | 21.22 s |
| tam/bottom_tabs | 26.22 s | **11.72 s** | 51.05 s | 31.69 s | 39.63 s | 20.60 s |
| tam/broken_accessibility | 27.57 s | **11.23 s** | 54.61 s | 34.53 s | 38.97 s | 27.48 s |
| tam/choice_controls | 32.72 s | **12.73 s** | 63.69 s | 38.88 s | 44.82 s | 24.56 s |
| tam/control_inventory | 45.54 s | **24.84 s** | 112.12 s | 49.72 s | — | 42.14 s |
| tam/debounced_search | 31.94 s | **16.37 s** | 69.34 s | — | 42.79 s | 32.39 s |
| tam/error_recovery | 31.87 s | **14.31 s** | 58.18 s | 38.74 s | 45.39 s | 25.61 s |
| tam/flattened_login | 30.26 s | **16.65 s** | 69.01 s | — | — | 33.07 s |
| tam/flattened_registration_form | 36.68 s | **23.18 s** | 89.57 s | — | — | 46.12 s |
| tam/gestures | 25.72 s | **10.10 s** | 53.89 s | 33.29 s | 39.84 s | — |
| tam/huge_virtualized_list | — | **26.02 s** | 216.11 s | 271.08 s | 149.51 s | 147.39 s |
| tam/infinite_scroll_list | — | **21.43 s** | 102.10 s | 109.28 s | 76.59 s | 57.12 s |
| tam/login_form | 31.54 s | **18.28 s** | 78.50 s | — | — | 34.85 s |
| tam/modal_flow | 29.12 s | **11.19 s** | 56.28 s | 35.52 s | 40.15 s | — |
| tam/permission_prompt | 29.91 s | **16.52 s** | 55.42 s | 34.22 s | 40.33 s | — |
| tam/photo_picker | 36.24 s | **17.68 s** | 59.55 s | 39.81 s | 47.02 s | 28.84 s |
| tam/product_catalog | 30.46 s | **10.79 s** | 59.06 s | 35.94 s | 44.26 s | 23.45 s |
| tam/sequential_onboarding | 50.58 s | **26.76 s** | 102.36 s | — | 58.15 s | 49.65 s |
| tam/sticky_chrome_target | 21.74 s | **6.23 s** | — | 30.14 s | 31.17 s | 19.07 s |
| tam/text_input_variations | 34.54 s | **20.30 s** | 82.94 s | 45.14 s | 48.01 s | 40.91 s |
| tam/vision_only | 26.17 s | **9.14 s** | 49.64 s | 32.40 s | 38.66 s | — |
| tam/webview_accessibility | 34.98 s | **17.35 s** | 63.25 s | — | 42.64 s | 31.47 s |
| tam/Whole app suite (sum) | — | **361.32 s** | — | — | — | — |
| tam/App flows passed | 88% | **98%** | **96%** | 78% | 80% | 82% |

</details>

### iOS (macOS runner)

![iOS (macOS runner)](docs/bench/ios-macos-runner.svg)

bench (iOS 26.2 simulator); GitHub macos-26 runner (Apple Silicon, 3 vCPU, 7 GB).

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 35 | 13 | 0 | 48 |
| [speedrun](https://github.com/hubastard/speedrun) | 18 | 27 | 3 | 48 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 3 | 17 | 22 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 0 | 7 | 20 | 27 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 0 | 0 | 3 | 3 |

![iOS (macOS runner): every scenario](docs/bench/ios-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | e2e |
|---|---:|---:|---:|---:|---:|
| Install size | 48.7 MB | 4.9 MB | **2.3 MB** | 396.3 MB | 41.6 MB |
| CLI start (--version) | 13 ms | **3 ms** | 4 ms | 3630 ms | 97 ms |
| Invalid flow reported | 19 ms | **3 ms** | 5 ms | 3228 ms | — |
| One-command flow (connect + run) | 5.09 s | 1.44 s | **1.08 s** | 50.25 s | 9.50 s |
| App launch | 17.38 s | 9.40 s | **7.17 s** | 83.03 s | 28.67 s |
| Navigation | 19.06 s | **12.64 s** | 14.36 s | 52.34 s | 25.44 s |
| Scroll to element | 28.03 s | 15.46 s | **13.89 s** | 115.24 s | 43.03 s |
| Text input | 29.50 s | — | **21.09 s** | — | 51.26 s |
| Assertion burst | 15.07 s | 8.98 s | **6.20 s** | 82.46 s | 28.11 s |
| Repeated taps | 32.74 s | **21.02 s** | 22.71 s | 84.07 s | 34.12 s |
| Whole suite (sum of medians) | 141.77 s | — | **85.42 s** | — | 210.62 s |
| Peak host memory | 29.9 MB | **20.8 MB** | **20.9 MB** | 224.3 MB | 138.2 MB |
| Host CPU time, whole suite | 22.68 s | 6.15 s | **1.62 s** | 149.56 s | 14.89 s |
| Runs passed | 93% | 83% | 80% | 77% | **100%** |
| Run-to-run variation | 10.6% (SD 2571 ms) | **7.3%** (SD 1046 ms) | 11.2% (SD 1934 ms) | 15.3% (SD 13059 ms) | 8.4% (SD 2735 ms) |
| app/animations | 134.85 s | 67.06 s | **48.03 s** | — | 57.48 s |
| app/appearance | — | **14.03 s** | 14.63 s | 85.96 s | 88.51 s |
| app/cart | — | **27.03 s** | 28.18 s | — | 56.54 s |
| app/chat | — | 36.41 s | **29.55 s** | — | 43.09 s |
| app/deep_links | 42.16 s | 30.45 s | 31.03 s | 77.55 s | **19.48 s** |
| app/device | — | 65.56 s | **45.54 s** | 187.45 s | — |
| app/drag | — | 57.27 s | **42.79 s** | — | — |
| app/feed | — | 70.01 s | **51.60 s** | — | — |
| app/gestures | 27.75 s | **14.04 s** | 16.37 s | 95.12 s | 37.64 s |
| app/inputs | — | — | — | — | — |
| app/native | — | **30.32 s** | **30.43 s** | — | — |
| app/onboarding | — | 21.13 s | **17.95 s** | — | — |
| app/popups | — | 36.94 s | **32.61 s** | 73.23 s | 52.46 s |
| app/profile | — | **31.96 s** | — | — | — |
| app/remember_me | 62.01 s | 33.89 s | **28.72 s** | 135.08 s | — |
| app/scripting | 32.62 s | 20.60 s | **18.58 s** | 65.69 s | 22.32 s |
| app/selectors | — | 34.15 s | **27.12 s** | — | — |
| app/shop_checkout | — | **77.15 s** | 87.59 s | — | 175.21 s |
| app/sign_in | 28.69 s | 23.79 s | **22.82 s** | — | — |
| app/timing | 33.55 s | **19.06 s** | 21.84 s | — | 35.06 s |
| app/Whole app suite (sum) | — | — | — | — | — |
| app/App flows passed | 29% | **95%** | 89% | 39% | 52% |
| tam/async_states | 35.40 s | 22.04 s | **19.78 s** | 93.25 s | 25.05 s |
| tam/bottom_tabs | 14.96 s | 7.20 s | **6.66 s** | 51.06 s | 17.81 s |
| tam/broken_accessibility | — | — | — | — | — |
| tam/choice_controls | 16.66 s | **7.25 s** | 8.68 s | 63.25 s | 16.41 s |
| tam/control_inventory | — | — | — | — | — |
| tam/debounced_search | 22.55 s | 15.68 s | **13.14 s** | 88.95 s | 27.75 s |
| tam/error_recovery | 26.67 s | 10.30 s | **9.55 s** | 96.12 s | 21.18 s |
| tam/flattened_login | — | — | — | — | — |
| tam/flattened_registration_form | — | — | — | — | — |
| tam/gestures | — | 11.55 s | **10.42 s** | — | — |
| tam/huge_virtualized_list | 181.43 s | 59.53 s | **48.15 s** | 300.01 s | 200.54 s |
| tam/infinite_scroll_list | 51.15 s | 19.30 s | **16.00 s** | 140.40 s | 49.81 s |
| tam/login_form | 30.74 s | — | **13.43 s** | — | — |
| tam/modal_flow | 16.41 s | **8.11 s** | **8.21 s** | 56.25 s | 14.51 s |
| tam/permission_prompt | — | 16.13 s | **14.95 s** | 100.18 s | — |
| tam/photo_picker | 31.06 s | 19.21 s | **15.83 s** | 94.15 s | 33.24 s |
| tam/product_catalog | 22.09 s | 8.77 s | **8.15 s** | 56.37 s | 19.73 s |
| tam/sequential_onboarding | 38.69 s | **21.17 s** | 29.36 s | — | 46.37 s |
| tam/sticky_chrome_target | 22.00 s | **11.79 s** | — | 85.72 s | 22.10 s |
| tam/text_input_variations | 37.89 s | — | **14.31 s** | — | 45.64 s |
| tam/vision_only | — | — | — | — | — |
| tam/webview_accessibility | — | — | — | — | — |
| tam/Whole app suite (sum) | — | — | — | — | — |
| tam/App flows passed | 64% | 62% | **68%** | 54% | 60% |

</details>
<!-- benchmark:end -->

## Web benchmark

virtuoso also drives web pages (`--platform web`: Chrome, headless), and has a benchmark of its
own there: Tester Army's web benchmark app (`tester-army/e2e`, `apps/web-benchmark`, served from
the same pinned commit as the mobile one), 34 flows in `bench/web-flows` in Maestro's syntax
(eight of its scenarios left out, each with its reason in `bench/web-flows/README.md`: a canvas,
hover, a file download and the like have no Maestro command). The field is virtuoso and Maestro,
which run the flows as written, against Playwright, Puppeteer, Selenium, Cypress and Tester
Army's e2e, each running its own port of every flow (`bench/web/<tool>`), written the way a user
of that tool writes it. Every tool drives the same Chrome for Testing build, headless, in a
1280 × 800 window, natively on each GitHub runner (`.github/workflows/web-bench.yml`); five runs
per tool and flow, the tools taking turns.

The code tools pass everything but Cypress's deferred dialog (its double click reaches the page as
one task, so the page opens two confirms). Maestro's web support has no iframes, browser dialogs,
second tabs, Tab key, or focus and enabled checks, and fails the flows that need them (and a few
whose scrolls never find their element, or whose list rebuilds under its tap). Its own Chrome is
not the pinned one.

<!-- benchmark-web:start -->
Each of the 126 performance events is a medal event. A performance metric is an event of its own on each platform and host (Chrome (Linux runner), Chrome (Windows runner), Chrome (macOS runner)), so a tool that wins on one and not another shows it. Written by `virtuoso-bench readme`, from GitHub Actions runs.

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 119 | 3 | 1 | 123 |
| [playwright](https://github.com/microsoft/playwright) | 11 | 40 | 37 | 88 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 8 | 64 | 36 | 108 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 7 | 19 | 41 | 67 |
| [e2e](https://github.com/tester-army/e2e) | 6 | 2 | 4 | 12 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 3 | 0 | 0 | 3 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 6 | 4 | 10 |

### Chrome (Linux runner)

![Chrome (Linux runner)](docs/bench/chrome-linux-runner.svg)

Chrome for Testing 158.0.8127.88, headless, 1280 x 800; GitHub Linux runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 1 | 0 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 4 | 23 | 30 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 2 | 34 | 2 | 38 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 3 | 20 | 25 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 1 | 0 | 3 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 1 | 3 |

![Chrome (Linux runner): every scenario](docs/bench/chrome-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.8 MB** | 396.3 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 73.6 MB |
| CLI start (--version) | **2 ms** | 1274 ms | 322 ms | 132 ms | 85 ms | 26 ms | 26 ms |
| Peak host memory | 233.2 MB | 301.0 MB | **228.4 MB** | 242.5 MB | 236.7 MB | 468.8 MB | 259.0 MB |
| Host CPU time, whole suite | **23.95 s** | 648.82 s | 40.95 s | 35.76 s | 52.71 s | 417.50 s | 159.57 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |
| Run-to-run variation | 4.6% (SD 27 ms) | **1.1%** (SD 146 ms) | 3.1% (SD 37 ms) | 4.2% (SD 42 ms) | 3.3% (SD 36 ms) | 1.8% (SD 99 ms) | 21.2% (SD 687 ms) |
| aria_hidden_flow | **0.44 s** | 11.19 s | 1.03 s | 0.89 s | 1.00 s | 5.51 s | 2.74 s |
| async_states | **2.38 s** | — | 3.13 s | 2.71 s | 2.89 s | 6.44 s | 5.75 s |
| cart_totals | **0.45 s** | 11.96 s | 1.05 s | 0.97 s | 1.05 s | 5.31 s | 2.83 s |
| checkout_review | **0.35 s** | 8.73 s | 0.80 s | 0.72 s | 0.81 s | 4.28 s | 2.32 s |
| context_menu_trap | **0.49 s** | 18.75 s | 1.12 s | 1.05 s | 1.19 s | 5.75 s | 3.07 s |
| control_inventory | **1.10 s** | — | 1.95 s | 1.69 s | 2.17 s | 5.90 s | 3.52 s |
| date_picker | **0.51 s** | 20.23 s | 1.20 s | 1.03 s | 1.22 s | 5.87 s | 3.29 s |
| debounced_search | **1.60 s** | 11.97 s | 2.10 s | 1.93 s | 2.03 s | 5.53 s | 4.95 s |
| deferred_dialog | **1.33 s** | — | 2.28 s | 1.75 s | 2.02 s | — | 5.18 s |
| div_soup | **0.43 s** | 18.63 s | 1.01 s | 1.01 s | 1.01 s | 4.72 s | 4.07 s |
| filter_deep_link | **0.45 s** | 9.69 s | 1.00 s | 0.94 s | 0.97 s | 5.53 s | 2.81 s |
| gift_card_purchase | **0.40 s** | — | 0.90 s | 0.85 s | 0.95 s | 4.59 s | 2.63 s |
| iframe_form | **0.47 s** | — | 1.12 s | 0.96 s | 1.17 s | 5.64 s | 3.10 s |
| infinite_scroll | **2.43 s** | — | 3.15 s | 3.05 s | 3.14 s | 6.98 s | 4.94 s |
| kanban_board | **0.76 s** | 12.12 s | 1.21 s | 1.07 s | 1.84 s | 5.98 s | 3.02 s |
| login_form | **0.43 s** | 17.20 s | 0.89 s | 0.82 s | 0.97 s | 4.63 s | 3.91 s |
| lying_labels | **0.48 s** | 14.08 s | 1.07 s | 0.95 s | 1.11 s | 5.75 s | 3.01 s |
| native_dialogs | **0.46 s** | — | 0.84 s | 0.85 s | 0.78 s | 4.30 s | 2.43 s |
| new_tab_flow | **0.50 s** | — | 1.16 s | 0.99 s | 1.22 s | 5.60 s | 2.84 s |
| newsletter_signup | **0.38 s** | 13.84 s | 0.83 s | 0.79 s | 0.86 s | 4.37 s | 2.23 s |
| onboarding_wizard | **1.12 s** | — | 2.28 s | 1.84 s | 2.99 s | 9.27 s | 6.24 s |
| order_history | **0.39 s** | 11.87 s | 0.89 s | 0.75 s | 0.87 s | 4.46 s | 2.49 s |
| otp_auto_advance | **0.78 s** | — | 1.82 s | 1.22 s | 1.40 s | 5.95 s | 4.67 s |
| overlay_trap | **0.98 s** | 11.71 s | 1.64 s | 1.27 s | 1.45 s | 5.03 s | 3.73 s |
| playbook_cleanup | **0.53 s** | 19.00 s | 1.14 s | 1.06 s | 1.20 s | 5.79 s | 2.94 s |
| price_sorting | **0.33 s** | 7.44 s | 0.78 s | 0.68 s | 0.79 s | 4.18 s | 2.12 s |
| product_reviews | **0.38 s** | 8.72 s | 1.01 s | 0.85 s | 0.93 s | 5.32 s | 2.71 s |
| promo_storefront | **1.54 s** | 16.44 s | 2.19 s | 1.88 s | 1.97 s | 5.63 s | 3.44 s |
| saved_addresses | **0.45 s** | 11.78 s | 1.03 s | 0.97 s | 1.03 s | 5.48 s | 3.00 s |
| shadow_dom_form | **0.39 s** | — | 0.93 s | 0.83 s | 1.00 s | 4.53 s | 2.45 s |
| stale_dom | **0.48 s** | — | 1.16 s | 0.97 s | 1.10 s | 5.84 s | 4.33 s |
| sticky_chrome | **0.38 s** | — | 0.80 s | 0.73 s | 0.80 s | 4.31 s | 2.23 s |
| virtualized_table | **1.36 s** | — | 7.80 s | 14.37 s | 14.57 s | 60.67 s | 25.85 s |
| wishlist | **0.41 s** | 14.95 s | 0.95 s | 0.86 s | 0.97 s | 4.65 s | 2.54 s |
| Whole app suite (sum) | **25.37 s** | — | 52.26 s | 53.27 s | 59.48 s | — | 137.39 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |

</details>

### Chrome (Windows runner)

![Chrome (Windows runner)](docs/bench/chrome-windows-runner.svg)

Chrome for Testing 158.0.8127.88, headless, 1280 x 800; GitHub Windows runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 38 | 2 | 1 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 5 | 29 | 5 | 39 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 4 | 7 | 22 | 33 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 1 | 2 | 5 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 0 | 10 | 12 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 2 | 4 |

![Chrome (Windows runner): every scenario](docs/bench/chrome-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.4 MB** | 396.3 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 78.4 MB |
| CLI start (--version) | **4 ms** | 1338 ms | 283 ms | 150 ms | 105 ms | 34 ms | 35 ms |
| Peak host memory | **6.4 MB** | 265.5 MB | 96.1 MB | 58.6 MB | 50.9 MB | 36.4 MB | 38.1 MB |
| Host CPU time, whole suite | **52.36 s** | 857.36 s | 97.00 s | 92.83 s | 106.58 s | 815.86 s | 254.70 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |
| Run-to-run variation | 12.1% (SD 284 ms) | **3.2%** (SD 559 ms) | 11.8% (SD 303 ms) | 10.7% (SD 268 ms) | 6.9% (SD 166 ms) | 5.5% (SD 632 ms) | 4.9% (SD 217 ms) |
| aria_hidden_flow | **1.45 s** | 15.61 s | 1.77 s | 1.92 s | 2.03 s | 10.23 s | 3.73 s |
| async_states | 4.19 s | — | 3.87 s | **3.78 s** | 4.67 s | 10.38 s | 5.33 s |
| cart_totals | **1.47 s** | 16.72 s | 1.90 s | 2.06 s | 2.02 s | 9.95 s | 3.83 s |
| checkout_review | **1.60 s** | 13.91 s | 2.05 s | 2.26 s | 2.63 s | 10.40 s | 3.63 s |
| context_menu_trap | **1.45 s** | 24.51 s | 1.78 s | 2.28 s | 2.11 s | 10.20 s | 3.95 s |
| control_inventory | **2.05 s** | — | **2.02 s** | 2.47 s | 2.84 s | 7.87 s | 3.31 s |
| date_picker | **1.49 s** | 27.91 s | 1.79 s | 1.97 s | 2.21 s | 10.24 s | 3.98 s |
| debounced_search | 3.07 s | 15.47 s | **2.68 s** | 3.01 s | 3.48 s | 9.70 s | 4.23 s |
| deferred_dialog | **2.32 s** | — | 2.75 s | 2.49 s | 3.05 s | — | 4.69 s |
| div_soup | **1.78 s** | 28.61 s | 1.97 s | 2.30 s | 2.45 s | 10.73 s | 4.09 s |
| filter_deep_link | **1.39 s** | 13.13 s | 1.73 s | 1.93 s | 1.97 s | 10.05 s | 3.58 s |
| gift_card_purchase | **1.24 s** | — | 1.40 s | 1.61 s | 1.69 s | 6.50 s | 2.43 s |
| iframe_form | **1.49 s** | — | 1.82 s | 2.00 s | 2.10 s | 10.25 s | 4.08 s |
| infinite_scroll | **3.41 s** | — | **3.41 s** | 3.88 s | 4.46 s | 11.40 s | 5.64 s |
| kanban_board | **1.51 s** | 16.55 s | 1.90 s | 1.90 s | 2.89 s | 10.49 s | 4.03 s |
| login_form | **1.67 s** | 25.51 s | 2.02 s | 2.32 s | 2.35 s | 10.68 s | 3.80 s |
| lying_labels | **1.34 s** | 19.67 s | 1.76 s | 1.88 s | 2.03 s | 10.20 s | 3.73 s |
| native_dialogs | **1.18 s** | — | 1.41 s | 1.68 s | 1.58 s | 6.65 s | 2.45 s |
| new_tab_flow | **1.46 s** | — | 1.79 s | 2.02 s | 2.15 s | 10.30 s | 3.89 s |
| newsletter_signup | **1.41 s** | 18.21 s | 1.71 s | 1.76 s | 1.82 s | 8.13 s | 2.85 s |
| onboarding_wizard | **2.20 s** | — | 2.69 s | 2.60 s | 4.40 s | 14.00 s | 6.04 s |
| order_history | **1.63 s** | 19.37 s | 1.95 s | 2.14 s | 2.27 s | 10.48 s | 3.76 s |
| otp_auto_advance | **1.64 s** | — | 2.25 s | 1.92 s | 2.29 s | 10.31 s | 3.93 s |
| overlay_trap | **1.66 s** | 13.76 s | 1.77 s | 1.75 s | 2.26 s | 6.85 s | 2.91 s |
| playbook_cleanup | **1.42 s** | 25.24 s | 1.83 s | 2.03 s | 2.16 s | 10.39 s | 3.89 s |
| price_sorting | **1.47 s** | 10.21 s | 1.79 s | 2.07 s | 1.84 s | 7.95 s | 2.77 s |
| product_reviews | **1.37 s** | 12.21 s | 1.80 s | 1.87 s | 1.90 s | 10.01 s | 3.66 s |
| promo_storefront | 2.85 s | 24.21 s | 2.92 s | **2.79 s** | 3.42 s | 11.65 s | 4.81 s |
| saved_addresses | **1.36 s** | 16.57 s | 1.81 s | 2.08 s | 1.98 s | 10.23 s | 3.77 s |
| shadow_dom_form | **1.24 s** | — | 1.49 s | 1.71 s | 1.90 s | 6.85 s | 2.54 s |
| stale_dom | **1.41 s** | — | 1.82 s | 2.14 s | 2.12 s | 10.35 s | 3.90 s |
| sticky_chrome | **1.45 s** | — | 1.68 s | 1.87 s | 1.86 s | 8.23 s | 2.93 s |
| virtualized_table | **4.58 s** | — | 8.32 s | 15.21 s | 15.85 s | 82.40 s | 25.65 s |
| wishlist | **1.67 s** | 22.58 s | 1.99 s | 2.15 s | 2.30 s | 10.70 s | 3.98 s |
| Whole app suite (sum) | **62.93 s** | — | 75.63 s | 87.87 s | 97.11 s | — | 151.79 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |

</details>

### Chrome (macOS runner)

![Chrome (macOS runner)](docs/bench/chrome-macos-runner.svg)

Chrome for Testing 158.0.8127.88, headless, 1280 x 800; GitHub macOS runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 41 | 0 | 0 | 41 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 3 | 16 | 11 | 30 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 7 | 9 | 19 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 2 | 23 | 12 | 37 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 0 | 2 | 4 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 1 | 3 |

![Chrome (macOS runner): every scenario](docs/bench/chrome-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.3 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 66.6 MB |
| CLI start (--version) | **3 ms** | 1054 ms | 323 ms | 144 ms | 80 ms | 36 ms | 38 ms |
| Peak host memory | **183.4 MB** | 261.4 MB | **180.6 MB** | 190.8 MB | **186.7 MB** | 452.1 MB | 221.0 MB |
| Host CPU time, whole suite | **63.90 s** | 674.95 s | 112.90 s | 113.52 s | 66.21 s | 347.51 s | 208.76 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **100%** | 96% | **100%** |
| Run-to-run variation | 14.8% (SD 237 ms) | **8.5%** (SD 2807 ms) | 16.1% (SD 497 ms) | 16.4% (SD 421 ms) | 14.1% (SD 351 ms) | 12.9% (SD 1297 ms) | 13.2% (SD 687 ms) |
| aria_hidden_flow | **1.57 s** | 23.83 s | 3.10 s | 2.41 s | 2.65 s | 10.87 s | 5.44 s |
| async_states | **3.96 s** | — | 5.54 s | 4.44 s | 4.47 s | 14.55 s | 6.60 s |
| cart_totals | **0.97 s** | 18.05 s | 1.99 s | 1.58 s | 1.65 s | 7.37 s | 3.68 s |
| checkout_review | **1.37 s** | 22.15 s | 4.19 s | 2.90 s | 2.62 s | 12.68 s | 5.28 s |
| context_menu_trap | **1.58 s** | 49.11 s | 3.36 s | 2.38 s | 2.63 s | 12.72 s | 5.33 s |
| control_inventory | **3.19 s** | — | 4.96 s | 4.90 s | 4.85 s | 17.15 s | 6.89 s |
| date_picker | **1.73 s** | 48.03 s | 3.87 s | 3.08 s | 2.56 s | 14.19 s | 6.18 s |
| debounced_search | **2.67 s** | 22.59 s | 4.08 s | 3.19 s | 3.56 s | 10.56 s | 5.55 s |
| deferred_dialog | **2.05 s** | — | 3.57 s | 2.57 s | 2.57 s | — | 5.07 s |
| div_soup | **2.03 s** | 47.50 s | 4.01 s | 3.86 s | 2.89 s | 13.52 s | 6.58 s |
| filter_deep_link | **1.33 s** | 19.20 s | 2.59 s | 1.89 s | 2.73 s | 11.46 s | 4.86 s |
| gift_card_purchase | **1.43 s** | — | 2.44 s | 2.22 s | 2.11 s | 9.37 s | 5.82 s |
| iframe_form | **1.46 s** | — | 2.96 s | 2.52 s | 2.55 s | 10.23 s | 5.13 s |
| infinite_scroll | **3.84 s** | — | 5.50 s | 5.63 s | 5.79 s | 11.55 s | 9.24 s |
| kanban_board | **1.77 s** | 19.92 s | 2.08 s | 2.06 s | 2.86 s | 6.65 s | 4.03 s |
| login_form | **1.71 s** | 46.26 s | 2.42 s | 2.67 s | 2.96 s | 11.33 s | 5.12 s |
| lying_labels | **1.31 s** | 24.77 s | 2.40 s | 2.04 s | 2.21 s | 9.69 s | 5.06 s |
| native_dialogs | **1.29 s** | — | 2.78 s | 2.63 s | 2.62 s | 11.69 s | 5.56 s |
| new_tab_flow | **1.39 s** | — | 2.74 s | 2.14 s | 2.50 s | 8.37 s | 4.85 s |
| newsletter_signup | **1.57 s** | 42.05 s | 2.50 s | 2.08 s | 2.14 s | 8.42 s | 5.12 s |
| onboarding_wizard | **3.94 s** | — | 5.54 s | 5.50 s | 4.85 s | 16.25 s | 11.49 s |
| order_history | **1.14 s** | 23.67 s | 2.32 s | 1.90 s | 1.90 s | 7.84 s | 4.87 s |
| otp_auto_advance | **1.97 s** | — | 3.66 s | 2.44 s | 2.96 s | 10.78 s | 5.75 s |
| overlay_trap | **1.95 s** | 23.07 s | 2.73 s | 2.61 s | 2.80 s | 10.73 s | 5.49 s |
| playbook_cleanup | **1.44 s** | 44.03 s | 2.67 s | 2.16 s | 2.08 s | 8.94 s | 4.73 s |
| price_sorting | **1.19 s** | 16.78 s | 2.20 s | 2.16 s | 2.70 s | 9.24 s | 4.71 s |
| product_reviews | **0.90 s** | 15.90 s | 2.47 s | 2.48 s | 1.86 s | 9.54 s | 5.30 s |
| promo_storefront | **2.49 s** | 45.46 s | 3.20 s | 3.02 s | 3.53 s | 10.33 s | 6.08 s |
| saved_addresses | **1.35 s** | 23.47 s | 2.30 s | 2.71 s | 2.22 s | 9.87 s | 5.11 s |
| shadow_dom_form | **1.30 s** | — | 2.46 s | 2.44 s | 2.67 s | 10.11 s | 6.03 s |
| stale_dom | **1.20 s** | — | 2.30 s | 1.91 s | 2.08 s | 8.01 s | 4.51 s |
| sticky_chrome | **1.24 s** | — | 1.99 s | 1.71 s | 2.01 s | 8.50 s | 4.78 s |
| virtualized_table | **4.60 s** | — | 42.02 s | 43.39 s | 46.88 s | 49.14 s | 80.84 s |
| wishlist | **1.22 s** | 44.14 s | 2.28 s | 2.11 s | 1.98 s | 8.15 s | 4.45 s |
| Whole app suite (sum) | **64.16 s** | — | 145.22 s | 133.71 s | 139.46 s | — | 265.51 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **100%** | 96% | **100%** |

</details>
<!-- benchmark-web:end -->

### Web benchmark on phones

Two more fields run the same flows on a phone's screen. **Pixel 7 (emulated)**: desktop Chrome
emulating a Pixel 7 (412 × 839, touch, a phone's user agent), each tool its own way: virtuoso
`--emulate pixel-7`, Playwright's and Puppeteer's device emulation, chromedriver's mobile
emulation for Selenium, Cypress and e2e a viewport and user agent (neither emulates touch),
Maestro's web mode a window of that size (all it has). **Chrome on Android**: the latest stable
Chrome milestone on a Pixel 7 emulator (API 36, no on-screen keyboard as in the other fields) on
GitHub's Linux runners (`.github/workflows/web-android-bench.yml`): Chromium's own x86_64 build
of it (159.0.8154.0, from Chromium's snapshots; the image's Chrome is 133, and Google publishes
no x86_64 Chrome), every tool driving it: virtuoso `--device android`, Playwright's Android
support, Puppeteer over adb, Selenium and Appium through chromedriver's Android mode. Cypress
drives no device, and e2e's engine needs browser contexts Chrome on Android cannot make.
**Maestro is not measured there**: it reads a page on Android through the accessibility tree
(through Chrome the app; its web mode drives a desktop Chrome only), and from Chrome 158 an
ordinary tab builds that tree only while an accessibility service runs, while Maestro's driver
(its UiAutomation) suppresses every accessibility service for as long as it is connected. It
sees no page, with or without a service enabled (Maestro 2.12.1, the latest; checked on the
emulator). Until Chrome 157 it ran there, through Chrome the app. The emulator crashes every few minutes on those runners, whatever its image or renderer,
so each flow runs a round at a time (every tool once, in turn) and a round the emulator did not
live through is run again, whole, on the emulator started afresh (up to five times; a round it
never lived through is left out). Android's "isn't responding" dialogs are off: on the slow
emulator they covered the page for Maestro, which reads the screen. A flow whose swipes go
between screen points has a phone copy (`bench/web-flows/pixel-7/`), and the Android field has
copies of its own (`bench/web-flows/android/`), each saying what it changes: Chrome on Android
shows a tool that reads its accessibility tree (Maestro) no `data-testid`, splits a text like
"$54.00" in two, names a glyph button only by its `aria-label`, and its prompt's field takes
typing once tapped; there an element is found by its text, its name or its row instead.

On Chrome 133, what Maestro missed through Chrome the app was its own result, each for a reason
the page or Chrome gave it: content hidden from the accessibility tree (`aria_hidden_flow`), labels in
place of what a cell shows (`control_inventory`), accessible names made wrong on purpose
(`gift_card_purchase`, `lying_labels`), fields with no name or label at all (`otp_auto_advance`,
`onboarding_wizard`, which a phone fills with no Tab key; `date_picker`'s read-only field), a
menu that answers a mouse and not a touch (`playbook_cleanup`), swipe points that are device
pixels there (`kanban_board`), the page leaving Chrome's tree once a field is typed into or
tapped (`new_tab_flow`, `saved_addresses`), a toast gone before it reads it (`async_states`), a
list rebuilt faster than it taps (`stale_dom`), and elements in a scroll box that Chrome gives no
height, never wholly visible (`infinite_scroll`, `virtualized_table`, `sticky_chrome`). On a
phone virtuoso taps as a finger when a typed field has the focus (a mouse press there shifted the
page and its click fell beside the target), as the Puppeteer port does. Playwright's misses there
are its own too: on the emulator each of its wheel steps takes some 450 ms (its Android
connection), so `virtualized_table`'s 140 s take it two thirds down the table, and its checks
before a click (the button stable, in view) outlast `stale_dom`'s button, rebuilt every second.

<!-- benchmark-web-phone:start -->
Each of the 168 performance events is a medal event. A performance metric is an event of its own on each platform and host (Chrome as a Pixel 7 (Linux runner), Chrome as a Pixel 7 (Windows runner), Chrome as a Pixel 7 (macOS runner), Chrome on a Pixel 7 emulator (Linux runner)), so a tool that wins on one and not another shows it. Written by `virtuoso-bench readme`, from GitHub Actions runs.

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 159 | 3 | 2 | 164 |
| [playwright](https://github.com/microsoft/playwright) | 13 | 40 | 57 | 110 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 9 | 100 | 38 | 147 |
| [e2e](https://github.com/tester-army/e2e) | 6 | 5 | 2 | 13 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 4 | 0 | 0 | 4 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 15 | 53 | 70 |
| [appium](https://github.com/appium/appium) | 2 | 2 | 0 | 4 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 5 | 4 | 9 |

### Chrome as a Pixel 7 (Linux runner)

![Chrome as a Pixel 7 (Linux runner)](docs/bench/chrome-as-a-pixel-7-linux-runner.svg)

Chrome for Testing 158.0.8127.88, headless, emulating a pixel-7; GitHub Linux runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 41 | 0 | 0 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 3 | 28 | 34 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 2 | 34 | 2 | 38 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 1 | 0 | 3 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 0 | 2 | 11 | 13 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 1 | 3 |

![Chrome as a Pixel 7 (Linux runner): every scenario](docs/bench/chrome-as-a-pixel-7-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.8 MB** | 396.3 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 73.6 MB |
| CLI start (--version) | **1 ms** | 1127 ms | 285 ms | 112 ms | 73 ms | 24 ms | 24 ms |
| Peak host memory | **229.9 MB** | 300.6 MB | **228.0 MB** | 242.7 MB | 237.1 MB | 468.8 MB | 259.6 MB |
| Host CPU time, whole suite | **25.31 s** | 613.39 s | 38.51 s | 33.53 s | 46.87 s | 385.18 s | 146.12 s |
| Runs passed | **100%** | 56% | **100%** | **100%** | 96% | 98% | **100%** |
| Run-to-run variation | 6.1% (SD 29 ms) | **1.0%** (SD 127 ms) | 2.3% (SD 25 ms) | 2.9% (SD 26 ms) | 3.4% (SD 37 ms) | 1.9% (SD 90 ms) | 18.5% (SD 541 ms) |
| aria_hidden_flow | **0.46 s** | 11.53 s | 1.06 s | 0.96 s | 1.15 s | 5.54 s | 2.73 s |
| async_states | **2.37 s** | — | 3.13 s | 2.74 s | 3.01 s | 6.47 s | 4.39 s |
| cart_totals | **0.36 s** | 10.68 s | 0.89 s | 0.80 s | 0.96 s | 4.24 s | 2.49 s |
| checkout_review | **0.30 s** | 8.32 s | 0.72 s | 0.63 s | 0.73 s | 3.76 s | 2.06 s |
| context_menu_trap | **0.42 s** | 17.14 s | 0.93 s | 0.88 s | 1.14 s | 4.62 s | 2.55 s |
| control_inventory | **1.14 s** | — | 2.15 s | 1.84 s | — | 6.91 s | 5.22 s |
| date_picker | **0.54 s** | 20.31 s | 1.21 s | 1.02 s | 1.53 s | 5.79 s | 3.09 s |
| debounced_search | **1.60 s** | 11.87 s | 2.12 s | 1.98 s | 2.11 s | 5.62 s | 3.62 s |
| deferred_dialog | **1.25 s** | — | 2.13 s | 1.63 s | 1.87 s | — | 3.54 s |
| div_soup | **0.36 s** | 18.15 s | 0.90 s | 0.90 s | 1.24 s | 4.15 s | 2.33 s |
| filter_deep_link | **0.39 s** | 8.67 s | 0.81 s | 0.76 s | 0.82 s | 4.48 s | 2.56 s |
| gift_card_purchase | **0.47 s** | — | 1.05 s | 1.03 s | 1.22 s | 5.46 s | 2.80 s |
| iframe_form | **0.47 s** | — | 1.12 s | 0.97 s | 1.41 s | 5.51 s | 3.41 s |
| infinite_scroll | **2.45 s** | — | 3.17 s | 3.08 s | 2.99 s | 7.29 s | 5.21 s |
| kanban_board | **0.66 s** | — | 1.02 s | 0.91 s | 2.30 s | 4.71 s | 2.65 s |
| login_form | **0.36 s** | 16.77 s | 0.77 s | 0.69 s | 0.96 s | 4.08 s | 2.15 s |
| lying_labels | **0.40 s** | 12.48 s | 0.86 s | 0.79 s | 1.03 s | 4.56 s | 2.41 s |
| native_dialogs | **0.46 s** | — | 1.00 s | 0.87 s | 0.98 s | 5.26 s | 2.75 s |
| new_tab_flow | **0.49 s** | — | 1.17 s | 1.04 s | 1.30 s | 5.61 s | 2.89 s |
| newsletter_signup | **0.41 s** | 14.45 s | 0.89 s | 0.88 s | 0.98 s | 4.71 s | 2.55 s |
| onboarding_wizard | **0.94 s** | — | 1.99 s | 1.59 s | 4.63 s | 7.72 s | 4.23 s |
| order_history | **0.32 s** | 11.38 s | 0.79 s | 0.65 s | 0.90 s | 3.92 s | 2.15 s |
| otp_auto_advance | **0.74 s** | — | 1.64 s | 1.09 s | 1.28 s | 4.81 s | 2.69 s |
| overlay_trap | **0.91 s** | 12.63 s | 1.83 s | 1.37 s | 1.57 s | 5.87 s | 3.15 s |
| playbook_cleanup | **0.49 s** | 18.39 s | 1.11 s | 1.01 s | 1.38 s | 5.62 s | 2.79 s |
| price_sorting | **0.34 s** | 7.60 s | 0.80 s | 0.76 s | 0.80 s | 4.33 s | 2.25 s |
| product_reviews | **0.33 s** | 7.44 s | 0.78 s | 0.72 s | 0.77 s | 4.26 s | 2.29 s |
| promo_storefront | **1.49 s** | 15.82 s | 2.09 s | 1.78 s | 2.03 s | 5.00 s | 3.15 s |
| saved_addresses | **0.37 s** | 10.32 s | 0.87 s | 0.82 s | 0.97 s | 4.37 s | 2.42 s |
| shadow_dom_form | **0.49 s** | — | 1.05 s | 1.00 s | 1.41 s | 5.64 s | 2.85 s |
| stale_dom | **0.45 s** | — | 1.13 s | 0.92 s | 1.24 s | 5.56 s | 2.80 s |
| sticky_chrome | **0.38 s** | — | 0.84 s | 0.76 s | 0.86 s | 4.40 s | 3.86 s |
| virtualized_table | **1.38 s** | — | 10.98 s | 10.86 s | 3.19 s | 54.50 s | 24.06 s |
| wishlist | **0.35 s** | 14.05 s | 0.82 s | 0.70 s | 1.01 s | 3.90 s | 2.21 s |
| Whole app suite (sum) | **24.33 s** | — | 53.81 s | 48.41 s | — | — | 122.31 s |
| App flows passed | **100%** | 56% | **100%** | **100%** | 96% | 98% | **100%** |

</details>

### Chrome as a Pixel 7 (Windows runner)

![Chrome as a Pixel 7 (Windows runner)](docs/bench/chrome-as-a-pixel-7-windows-runner.svg)

Chrome for Testing 158.0.8127.88, headless, emulating a pixel-7; GitHub Windows runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 37 | 3 | 1 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 7 | 28 | 4 | 39 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 4 | 6 | 26 | 36 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 2 | 1 | 5 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 2 | 4 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 0 | 1 | 3 | 4 |

![Chrome as a Pixel 7 (Windows runner): every scenario](docs/bench/chrome-as-a-pixel-7-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.4 MB** | 396.3 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 78.4 MB |
| CLI start (--version) | **4 ms** | 1293 ms | 294 ms | 159 ms | 116 ms | 39 ms | 40 ms |
| Peak host memory | **6.4 MB** | 264.9 MB | 97.3 MB | 58.9 MB | 51.2 MB | 36.5 MB | 38.2 MB |
| Host CPU time, whole suite | **43.48 s** | 735.17 s | 83.20 s | 81.92 s | 93.94 s | 718.44 s | 222.53 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | 97% | 97% | **100%** |
| Run-to-run variation | 8.3% (SD 145 ms) | **2.3%** (SD 422 ms) | 9.9% (SD 205 ms) | 12.9% (SD 283 ms) | 16.7% (SD 648 ms) | 3.4% (SD 311 ms) | 2.7% (SD 87 ms) |
| aria_hidden_flow | **1.45 s** | 13.97 s | 1.72 s | 1.87 s | 2.13 s | 8.33 s | 3.11 s |
| async_states | **3.46 s** | — | **3.38 s** | **3.43 s** | 4.16 s | 9.16 s | 4.92 s |
| cart_totals | **1.43 s** | 16.77 s | 1.76 s | 2.01 s | 2.13 s | 9.95 s | 3.61 s |
| checkout_review | **1.41 s** | 12.18 s | 1.67 s | 1.83 s | 1.98 s | 8.10 s | 2.97 s |
| context_menu_trap | **1.52 s** | 24.55 s | 1.93 s | 2.10 s | 2.39 s | 10.30 s | 3.74 s |
| control_inventory | 2.11 s | — | **2.07 s** | 2.36 s | — | 8.14 s | 3.33 s |
| date_picker | **1.59 s** | 26.04 s | 1.73 s | 2.02 s | 2.52 s | 8.76 s | 3.30 s |
| debounced_search | 2.64 s | 13.41 s | **2.25 s** | 2.49 s | 3.12 s | 8.27 s | 3.79 s |
| deferred_dialog | **2.25 s** | — | 2.73 s | 2.50 s | 3.03 s | — | 4.42 s |
| div_soup | **1.45 s** | 26.17 s | 1.72 s | 2.04 s | 2.51 s | 8.58 s | 3.28 s |
| filter_deep_link | **1.46 s** | 12.96 s | 1.75 s | 1.98 s | 2.01 s | 9.89 s | 3.47 s |
| gift_card_purchase | **1.58 s** | — | 1.67 s | 2.01 s | 2.12 s | 6.74 s | 2.63 s |
| iframe_form | **1.52 s** | — | 1.68 s | 1.98 s | 2.36 s | 8.56 s | 3.42 s |
| infinite_scroll | 3.60 s | — | **3.34 s** | 3.74 s | 4.10 s | 10.01 s | 5.36 s |
| kanban_board | **1.51 s** | 16.23 s | 1.74 s | 2.00 s | 3.53 s | 10.36 s | 3.82 s |
| login_form | **1.56 s** | 24.27 s | 2.12 s | 1.96 s | 2.54 s | 8.57 s | 3.21 s |
| lying_labels | **1.48 s** | 19.57 s | 1.84 s | 2.09 s | 2.18 s | 10.11 s | 3.64 s |
| native_dialogs | **1.38 s** | — | 1.43 s | 1.71 s | 1.79 s | 6.56 s | 2.40 s |
| new_tab_flow | **1.52 s** | — | 1.74 s | 1.95 s | 2.25 s | 8.55 s | 3.24 s |
| newsletter_signup | **1.56 s** | 17.52 s | 1.69 s | 1.92 s | 2.08 s | 7.50 s | 2.70 s |
| onboarding_wizard | **2.06 s** | — | 2.64 s | 2.62 s | 6.57 s | 13.78 s | 5.58 s |
| order_history | **1.53 s** | 16.49 s | 1.72 s | 1.89 s | 2.23 s | 8.36 s | 3.04 s |
| otp_auto_advance | **1.74 s** | — | 2.21 s | 2.09 s | 2.41 s | 10.22 s | 3.88 s |
| overlay_trap | **1.83 s** | 14.26 s | **1.83 s** | 1.97 s | 2.41 s | 7.33 s | 2.98 s |
| playbook_cleanup | **1.42 s** | 23.46 s | 1.75 s | 1.83 s | 2.29 s | 8.59 s | 3.23 s |
| price_sorting | **1.36 s** | 9.64 s | 1.61 s | 1.92 s | 1.81 s | 7.03 s | 2.49 s |
| product_reviews | **1.33 s** | 12.04 s | 1.79 s | 2.00 s | 1.90 s | 9.90 s | 3.42 s |
| promo_storefront | 2.67 s | 21.94 s | 2.63 s | **2.41 s** | 3.25 s | 9.37 s | 4.11 s |
| saved_addresses | **1.43 s** | 15.89 s | 1.78 s | 2.06 s | 2.13 s | 9.97 s | 3.61 s |
| shadow_dom_form | **1.39 s** | — | 1.54 s | 1.76 s | 2.04 s | 6.63 s | 2.38 s |
| stale_dom | **1.47 s** | — | 1.76 s | 1.90 s | 2.13 s | 8.53 s | 3.21 s |
| sticky_chrome | **1.50 s** | — | 1.68 s | 1.89 s | 1.95 s | 7.23 s | 2.59 s |
| virtualized_table | **4.52 s** | — | 11.63 s | 11.81 s | 6.18 s | 76.17 s | 25.21 s |
| wishlist | **1.46 s** | 20.42 s | 1.68 s | 1.94 s | 2.22 s | 8.04 s | 3.16 s |
| Whole app suite (sum) | **62.19 s** | — | 76.25 s | 82.11 s | — | — | 139.24 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | 97% | 97% | **100%** |

</details>

### Chrome as a Pixel 7 (macOS runner)

![Chrome as a Pixel 7 (macOS runner)](docs/bench/chrome-as-a-pixel-7-macos-runner.svg)

Chrome for Testing 158.0.8127.88, headless, emulating a pixel-7; GitHub macOS runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 41 | 0 | 0 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 7 | 20 | 30 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 2 | 1 | 5 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 0 | 28 | 6 | 34 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 0 | 5 | 12 | 17 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 1 | 1 | 2 |

![Chrome as a Pixel 7 (macOS runner): every scenario](docs/bench/chrome-as-a-pixel-7-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.3 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 66.6 MB |
| CLI start (--version) | **4 ms** | 1430 ms | 319 ms | 176 ms | 114 ms | 42 ms | 42 ms |
| Peak host memory | **184.1 MB** | 261.9 MB | **180.7 MB** | 191.6 MB | 188.3 MB | 453.0 MB | 221.5 MB |
| Host CPU time, whole suite | **69.58 s** | 673.61 s | 111.31 s | 117.13 s | 168.07 s | 339.57 s | 208.68 s |
| Runs passed | **100%** | 56% | **100%** | 97% | 95% | 97% | **100%** |
| Run-to-run variation | 16.2% (SD 245 ms) | **7.4%** (SD 2419 ms) | 13.8% (SD 400 ms) | 17.0% (SD 413 ms) | 14.4% (SD 412 ms) | 14.4% (SD 1434 ms) | 11.3% (SD 547 ms) |
| aria_hidden_flow | **1.36 s** | 21.99 s | 2.65 s | 2.24 s | 2.70 s | 10.65 s | 4.98 s |
| async_states | **4.01 s** | — | 5.26 s | 4.68 s | 5.50 s | 14.93 s | 8.44 s |
| cart_totals | **1.46 s** | 24.82 s | 3.02 s | 2.37 s | 3.01 s | 10.38 s | 5.50 s |
| checkout_review | **1.64 s** | 21.33 s | 3.23 s | 2.41 s | 2.27 s | 10.79 s | 5.52 s |
| context_menu_trap | **1.64 s** | 47.56 s | 3.21 s | 2.66 s | 3.58 s | 12.76 s | 5.41 s |
| control_inventory | **3.18 s** | — | 6.42 s | — | 7.47 s | 17.10 s | 7.74 s |
| date_picker | **1.43 s** | 46.26 s | 2.92 s | 2.49 s | 3.43 s | 9.59 s | 5.30 s |
| debounced_search | **2.41 s** | 22.74 s | 4.06 s | 3.00 s | 3.54 s | 8.55 s | 5.08 s |
| deferred_dialog | **2.00 s** | — | 3.79 s | 2.92 s | 3.64 s | — | 6.12 s |
| div_soup | **1.53 s** | 48.21 s | 4.50 s | 3.99 s | 4.95 s | 13.75 s | 6.18 s |
| filter_deep_link | **1.53 s** | 20.17 s | 2.88 s | 2.52 s | 3.10 s | 12.31 s | 5.62 s |
| gift_card_purchase | **1.45 s** | — | 3.19 s | 3.28 s | 3.82 s | 9.72 s | 5.65 s |
| iframe_form | **1.31 s** | — | 2.61 s | 2.33 s | 2.68 s | 8.59 s | 4.35 s |
| infinite_scroll | **3.52 s** | — | 5.52 s | 5.85 s | 4.46 s | 10.32 s | 8.58 s |
| kanban_board | **1.70 s** | — | 2.68 s | 2.34 s | 5.40 s | 9.96 s | 4.94 s |
| login_form | **1.27 s** | 43.58 s | 1.59 s | 1.48 s | 1.95 s | 7.34 s | 3.74 s |
| lying_labels | **1.38 s** | 23.42 s | 2.66 s | 2.25 s | 2.62 s | 11.05 s | 4.85 s |
| native_dialogs | **1.18 s** | — | 2.75 s | 2.23 s | 2.31 s | 9.59 s | 5.36 s |
| new_tab_flow | **1.39 s** | — | 3.09 s | 2.71 s | 3.14 s | 8.97 s | 4.61 s |
| newsletter_signup | **1.03 s** | 25.18 s | 1.72 s | 1.60 s | 1.78 s | 6.80 s | 3.82 s |
| onboarding_wizard | **3.28 s** | — | 5.37 s | 4.81 s | 10.59 s | 11.71 s | 8.87 s |
| order_history | **0.92 s** | 19.36 s | 1.43 s | 1.31 s | 1.98 s | 5.47 s | 3.11 s |
| otp_auto_advance | **1.56 s** | — | 2.85 s | 2.46 s | 2.55 s | 8.15 s | 4.33 s |
| overlay_trap | **2.45 s** | 28.26 s | 3.42 s | 3.54 s | 3.77 s | 13.74 s | 7.74 s |
| playbook_cleanup | **1.45 s** | 45.12 s | 3.75 s | 2.73 s | 3.21 s | 11.99 s | 5.13 s |
| price_sorting | **0.76 s** | 13.33 s | 1.66 s | 1.50 s | 1.57 s | 5.90 s | 3.23 s |
| product_reviews | **0.94 s** | 14.09 s | 1.75 s | 1.54 s | 1.55 s | 6.55 s | 3.55 s |
| promo_storefront | **2.24 s** | 44.79 s | 3.25 s | 3.10 s | 3.08 s | 7.49 s | 5.23 s |
| saved_addresses | **1.28 s** | 21.43 s | 2.68 s | 2.38 s | 2.94 s | 8.66 s | 5.44 s |
| shadow_dom_form | **2.45 s** | — | 4.30 s | 3.67 s | 3.71 s | 13.20 s | 6.87 s |
| stale_dom | **1.40 s** | — | 2.93 s | 2.43 s | — | 9.06 s | 5.56 s |
| sticky_chrome | **1.05 s** | — | 1.79 s | 1.54 s | 1.74 s | 6.66 s | 3.49 s |
| virtualized_table | **5.50 s** | — | 37.19 s | 46.44 s | — | 53.95 s | 81.57 s |
| wishlist | **1.26 s** | 45.59 s | 2.59 s | 2.49 s | 2.83 s | 8.41 s | 4.54 s |
| Whole app suite (sum) | **62.97 s** | — | 142.72 s | — | — | — | 260.43 s |
| App flows passed | **100%** | 56% | **100%** | 97% | 95% | 97% | **100%** |

</details>

### Chrome on a Pixel 7 emulator (Linux runner)

![Chrome on a Pixel 7 emulator (Linux runner)](docs/bench/chrome-on-a-pixel-7-emulator-linux-runner.svg)

Chrome on a Pixel 7 emulator (API 36, Google APIs); GitHub ubuntu-latest runner (4 vCPU, 16 GB), Android emulator on KVM.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 0 | 1 | 41 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 3 | 32 | 4 | 39 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 7 | 27 | 36 |
| [appium](https://github.com/appium/appium) | 2 | 2 | 0 | 4 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [playwright](https://github.com/microsoft/playwright) | 0 | 2 | 5 | 7 |

![Chrome on a Pixel 7 emulator (Linux runner): every scenario](docs/bench/chrome-on-a-pixel-7-emulator-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | appium |
|---|---:|---:|---:|---:|---:|---:|
| Install size | **2.8 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 147.7 MB |
| CLI start (--version) | **1 ms** | 1104 ms | 246 ms | 101 ms | 67 ms | 67 ms |
| Peak host memory | **5.8 MB** | 415.7 MB | 139.8 MB | 83.2 MB | 69.7 MB | 178.1 MB |
| Host CPU time, whole suite | **2.33 s** | 693.06 s | 31.87 s | 17.73 s | 10.72 s | 88.21 s |
| Runs passed | **99%** | 53% | 96% | **100%** | **100%** | **100%** |
| Run-to-run variation | 5.0% (SD 256 ms) | **3.0%** (SD 637 ms) | 10.1% (SD 1179 ms) | 9.7% (SD 809 ms) | 4.1% (SD 339 ms) | 3.6% (SD 435 ms) |
| aria_hidden_flow | **6.10 s** | — | 17.82 s | 8.46 s | 9.67 s | 15.18 s |
| async_states | **8.17 s** | — | 17.61 s | 10.83 s | 11.22 s | 15.39 s |
| cart_totals | **4.37 s** | 17.54 s | 6.31 s | 5.71 s | 7.22 s | 10.54 s |
| checkout_review | **4.38 s** | 18.11 s | 7.14 s | 6.50 s | 7.88 s | 11.39 s |
| context_menu_trap | **5.06 s** | 22.93 s | 12.82 s | 7.70 s | 8.05 s | 11.36 s |
| control_inventory | **7.30 s** | — | 24.39 s | 9.55 s | 12.92 s | 16.87 s |
| date_picker | **5.45 s** | — | 16.86 s | 8.05 s | 9.68 s | 13.31 s |
| debounced_search | **7.24 s** | 25.75 s | 15.74 s | 9.80 s | 10.30 s | 13.85 s |
| deferred_dialog | **5.47 s** | 20.86 s | 12.12 s | 7.35 s | 8.27 s | 11.84 s |
| div_soup | **4.99 s** | 25.06 s | 15.91 s | 6.88 s | 9.75 s | 13.35 s |
| filter_deep_link | **4.46 s** | 16.18 s | 6.00 s | 5.92 s | 6.49 s | 10.07 s |
| gift_card_purchase | **6.26 s** | — | 15.84 s | 9.31 s | 9.74 s | 13.78 s |
| iframe_form | **5.42 s** | 22.35 s | 15.45 s | 7.86 s | 9.33 s | 12.27 s |
| infinite_scroll | **7.15 s** | — | 16.91 s | 9.01 s | 10.14 s | 14.42 s |
| kanban_board | **5.96 s** | — | 18.24 s | 7.66 s | 8.36 s | 12.10 s |
| login_form | **7.89 s** | 33.07 s | 18.86 s | 11.48 s | 10.90 s | 16.15 s |
| lying_labels | **5.12 s** | — | 10.46 s | 6.90 s | 7.58 s | 10.77 s |
| native_dialogs | **6.27 s** | 34.08 s | 15.38 s | 8.92 s | 9.97 s | 15.09 s |
| new_tab_flow | **7.03 s** | — | 15.98 s | 10.00 s | 10.30 s | 14.77 s |
| newsletter_signup | **4.72 s** | 21.06 s | 9.54 s | 7.27 s | 7.28 s | 9.87 s |
| onboarding_wizard | **14.53 s** | — | 63.65 s | 17.76 s | 33.17 s | 37.55 s |
| order_history | **4.64 s** | 19.03 s | 11.38 s | 6.23 s | 7.53 s | 10.93 s |
| otp_auto_advance | **6.07 s** | — | 12.96 s | 9.10 s | 8.15 s | 11.43 s |
| overlay_trap | **4.97 s** | 19.00 s | 6.97 s | 6.20 s | 7.87 s | 11.57 s |
| playbook_cleanup | **6.13 s** | — | 17.71 s | 8.75 s | 10.39 s | 15.96 s |
| price_sorting | **4.28 s** | 15.91 s | 6.94 s | 6.06 s | 7.21 s | 10.96 s |
| product_reviews | **4.01 s** | 13.89 s | 5.73 s | 5.51 s | 6.29 s | 9.24 s |
| promo_storefront | **5.37 s** | 21.83 s | 17.54 s | 7.29 s | 8.82 s | 12.03 s |
| saved_addresses | **5.17 s** | — | 15.20 s | 7.84 s | 8.78 s | 12.85 s |
| shadow_dom_form | **5.34 s** | 21.26 s | 12.24 s | 7.41 s | 8.47 s | 11.84 s |
| stale_dom | **4.81 s** | — | 26.43 s | 6.71 s | 8.81 s | 11.86 s |
| sticky_chrome | **4.41 s** | — | 9.57 s | 5.65 s | 6.48 s | 9.58 s |
| virtualized_table | 67.31 s | — | — | **53.64 s** | 64.43 s | 71.55 s |
| wishlist | **4.43 s** | 21.29 s | 13.21 s | 6.14 s | 8.41 s | 11.51 s |
| Whole app suite (sum) | **260.29 s** | — | — | 319.45 s | 379.86 s | 511.23 s |
| App flows passed | **99%** | 53% | 96% | **100%** | **100%** | **100%** |

</details>
<!-- benchmark-web-phone:end -->

## License

MIT, see [LICENSE](LICENSE). The faker word lists in `crates/virtuoso/src/datafaker` come from
Datafaker and keep its Apache 2.0 license.

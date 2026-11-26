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
| [virtuoso](https://github.com/marksmayo/virtuoso) | 198 | 19 | 4 | 221 |
| [speedrun](https://github.com/hubastard/speedrun) | 43 | 75 | 6 | 124 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 37 | 26 | 52 | 115 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 25 | 6 | 7 | 38 |
| [argent](https://github.com/software-mansion/argent) | 13 | 28 | 39 | 80 |
| [e2e](https://github.com/tester-army/e2e) | 4 | 8 | 37 | 49 |
| [appium](https://github.com/appium/appium) | 3 | 5 | 21 | 29 |

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 154 | 19 | 4 | 177 |
| [speedrun](https://github.com/hubastard/speedrun) | 21 | 74 | 4 | 99 |
| [argent](https://github.com/software-mansion/argent) | 13 | 28 | 39 | 80 |
| [e2e](https://github.com/tester-army/e2e) | 4 | 8 | 37 | 49 |
| [appium](https://github.com/appium/appium) | 3 | 5 | 21 | 29 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 2 | 25 | 49 | 76 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 2 | 3 | 3 | 8 |

### Android (Linux runner)

![Android (Linux runner)](docs/bench/android-linux-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub ubuntu-latest runner (4 vCPU, 16 GB), Android emulator on KVM.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 55 | 6 | 0 | 61 |
| [speedrun](https://github.com/hubastard/speedrun) | 7 | 49 | 2 | 58 |
| [argent](https://github.com/software-mansion/argent) | 4 | 1 | 14 | 19 |
| [e2e](https://github.com/tester-army/e2e) | 3 | 2 | 19 | 24 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 1 | 18 | 20 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 1 | 0 | 2 |
| [appium](https://github.com/appium/appium) | 1 | 0 | 3 | 4 |

![Android (Linux runner): every scenario](docs/bench/android-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 49.5 MB | 3.7 MB | **2.4 MB** | 361.5 MB | 147.7 MB | 42.8 MB | 243.4 MB |
| CLI start (--version) | 7 ms | 1 ms | **1 ms** | 2084 ms | 457 ms | 26 ms | 30 ms |
| Invalid flow reported | 8 ms | 1 ms | **1 ms** | 2083 ms | — | — | 158 ms |
| One-command flow (connect + run) | 2.35 s | 1.16 s | **0.50 s** | 11.75 s | 12.03 s | 7.78 s | 1.10 s |
| App launch | 6.06 s | 2.52 s | **2.07 s** | 15.46 s | 10.83 s | 11.83 s | 5.60 s |
| Navigation | 19.27 s | 8.17 s | **7.64 s** | 36.53 s | 25.52 s | 26.78 s | 20.83 s |
| Scroll to element | 17.12 s | 5.12 s | **4.75 s** | 26.63 s | 29.73 s | 20.06 s | 14.75 s |
| Text input | 14.46 s | 10.14 s | **7.54 s** | 49.26 s | 37.49 s | 21.99 s | 14.83 s |
| Assertion burst | 7.55 s | 2.93 s | **2.25 s** | 17.96 s | 11.76 s | 13.76 s | 6.91 s |
| Repeated taps | 25.66 s | **11.02 s** | **11.05 s** | 47.95 s | 31.89 s | 34.37 s | 26.61 s |
| Whole suite (sum of medians) | 90.11 s | 39.90 s | **35.29 s** | 193.78 s | 147.21 s | 128.80 s | 89.53 s |
| Peak host memory | 26.8 MB | 5.8 MB | **4.2 MB** | 647.8 MB | 177.9 MB | 152.5 MB | 94.8 MB |
| Host CPU time, whole suite | 1.93 s | 1.16 s | **0.03 s** | 111.45 s | 13.09 s | 9.35 s | 1.31 s |
| Peak device agent memory | 132.4 MB | 147.8 MB | 132.8 MB | 158.6 MB | 285.9 MB | — | **91.8 MB** |
| Runs passed | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** |
| Run-to-run variation | 5.3% | 5.5% | 5.6% | 3.6% | 4.1% | **3.3%** | 4.2% |
| app/animations | 195.04 s | 119.89 s | **52.45 s** | — | 131.38 s | 106.57 s | 199.16 s |
| app/appearance | 52.94 s | 15.79 s | **12.97 s** | 304.63 s | 80.23 s | 44.44 s | 54.33 s |
| app/cart | — | 19.61 s | **14.90 s** | — | — | 47.00 s | — |
| app/chat | 43.14 s | 22.59 s | **21.02 s** | 208.79 s | 52.75 s | 36.75 s | 52.85 s |
| app/deep_links | 50.63 s | 29.06 s | 29.48 s | 158.75 s | 34.32 s | 29.31 s | **26.46 s** |
| app/device | — | 36.94 s | **33.70 s** | 420.25 s | 103.87 s | — | — |
| app/drag | — | 30.50 s | **24.87 s** | — | 151.46 s | 91.64 s | 143.26 s |
| app/feed | — | 36.20 s | **23.66 s** | — | 307.52 s | 127.90 s | 260.07 s |
| app/gestures | 36.58 s | 12.61 s | **10.96 s** | 273.42 s | 66.02 s | 34.39 s | 51.54 s |
| app/inputs | — | — | **51.08 s** | — | — | — | — |
| app/native | — | 21.25 s | **20.15 s** | — | — | — | — |
| app/onboarding | — | 9.47 s | **7.94 s** | — | 43.03 s | 45.94 s | 67.59 s |
| app/popups | — | 32.21 s | **29.47 s** | 198.01 s | 70.31 s | 55.37 s | 50.47 s |
| app/profile | 120.06 s | 41.97 s | **39.56 s** | 499.57 s | 201.82 s | 76.95 s | 142.00 s |
| app/remember_me | — | 20.52 s | **19.29 s** | 346.88 s | 129.72 s | 47.94 s | 84.01 s |
| app/scripting | — | 15.26 s | **13.30 s** | 180.65 s | 40.95 s | 25.83 s | 34.24 s |
| app/selectors | — | 23.90 s | **16.15 s** | 166.33 s | 45.62 s | 30.90 s | — |
| app/shop_checkout | — | 82.83 s | **79.43 s** | — | — | 111.21 s | — |
| app/sign_in | 49.44 s | 14.46 s | **12.32 s** | 248.25 s | 79.19 s | 37.16 s | 61.81 s |
| app/timing | 58.03 s | 19.64 s | **18.66 s** | 270.12 s | 79.38 s | 45.52 s | 69.26 s |
| app/Whole app suite (sum) | — | — | **531.35 s** | — | — | — | — |
| app/App flows passed | 40% | 94% | **100%** | 60% | 75% | 84% | 70% |
| tam/async_states | 21.54 s | 18.12 s | **16.97 s** | 37.40 s | 21.48 s | 23.09 s | 19.09 s |
| tam/bottom_tabs | 10.50 s | 4.91 s | **4.29 s** | 29.09 s | 16.09 s | 17.47 s | 10.09 s |
| tam/broken_accessibility | 12.19 s | 5.63 s | **5.25 s** | 30.68 s | 18.66 s | 16.77 s | 13.65 s |
| tam/choice_controls | 13.21 s | 5.68 s | **5.54 s** | 32.41 s | 17.90 s | 18.50 s | 12.72 s |
| tam/control_inventory | 19.83 s | **15.55 s** | **15.34 s** | 71.90 s | — | 25.20 s | 26.14 s |
| tam/debounced_search | 10.91 s | 7.55 s | **6.95 s** | 33.15 s | 19.25 s | 15.81 s | 14.23 s |
| tam/error_recovery | 12.99 s | 7.79 s | **6.71 s** | 30.05 s | 18.91 s | 19.33 s | 13.89 s |
| tam/flattened_login | 11.92 s | **9.14 s** | 9.42 s | 40.90 s | 20.80 s | 16.69 s | 18.94 s |
| tam/flattened_registration_form | 13.41 s | **10.78 s** | 11.28 s | 46.72 s | 21.76 s | 16.56 s | 21.49 s |
| tam/gestures | 8.55 s | 5.27 s | **4.58 s** | 25.40 s | 14.59 s | 15.30 s | — |
| tam/huge_virtualized_list | — | 20.30 s | **16.50 s** | 226.22 s | 183.20 s | 104.10 s | 142.62 s |
| tam/infinite_scroll_list | — | 9.64 s | **8.62 s** | 65.22 s | 50.29 s | 33.45 s | 38.69 s |
| tam/login_form | 10.65 s | 7.13 s | **6.60 s** | 36.35 s | 18.48 s | 14.15 s | 16.70 s |
| tam/modal_flow | 10.75 s | 5.00 s | **4.50 s** | 24.91 s | 15.59 s | 16.04 s | — |
| tam/permission_prompt | 14.27 s | 10.65 s | 10.44 s | 27.18 s | 15.80 s | 14.54 s | **8.90 s** |
| tam/photo_picker | 14.42 s | 7.98 s | **6.73 s** | 30.70 s | 20.13 s | 21.06 s | 13.53 s |
| tam/product_catalog | 9.03 s | 3.76 s | **3.31 s** | 26.23 s | 14.60 s | 14.28 s | 10.39 s |
| tam/sequential_onboarding | 23.10 s | 14.53 s | **13.23 s** | 65.19 s | 35.87 s | 20.75 s | 28.23 s |
| tam/sticky_chrome_target | 17.78 s | 6.17 s | **4.94 s** | — | 22.14 s | 21.63 s | 15.67 s |
| tam/text_input_variations | 14.91 s | **12.71 s** | 12.99 s | 51.36 s | 22.50 s | 18.06 s | 21.34 s |
| tam/vision_only | 10.82 s | 4.78 s | **4.16 s** | 26.82 s | 15.67 s | 16.13 s | 8.92 s |
| tam/webview_accessibility | 19.31 s | 7.84 s | **7.60 s** | 34.31 s | 20.58 s | 16.53 s | 14.84 s |
| tam/Whole app suite (sum) | — | 200.91 s | **185.98 s** | — | — | 495.46 s | — |
| tam/App flows passed | 90% | **100%** | **100%** | 95% | 93% | **100%** | 89% |

</details>

### Android (Windows runner)

![Android (Windows runner)](docs/bench/android-windows-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub windows-latest runner (4 vCPU, 16 GB), Android emulator on WHPX.

Not measured: speedrun, which runs on Linux and macOS only, so here in WSL: under WSL 1 it finds no flows, and WSL 2 cannot reach the emulator's adb-forwarded ports on GitHub's Windows runners (mirrored networking is unsupported there).

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 59 | 1 | 2 | 62 |
| [argent](https://github.com/software-mansion/argent) | 4 | 26 | 14 | 44 |
| [appium](https://github.com/appium/appium) | 1 | 3 | 9 | 13 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 1 | 2 | 4 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 0 | 21 | 18 | 39 |
| [e2e](https://github.com/tester-army/e2e) | 0 | 5 | 8 | 13 |

![Android (Windows runner): every scenario](docs/bench/android-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|
| Install size | 59.0 MB | **2.2 MB** | 361.5 MB | 129.8 MB | 41.4 MB | 238.3 MB |
| CLI start (--version) | 22 ms | **7 ms** | 3583 ms | 1490 ms | 81 ms | 94 ms |
| Invalid flow reported | 73 ms | **7 ms** | 4767 ms | — | — | 520 ms |
| One-command flow (connect + run) | 11.44 s | **1.91 s** | 34.20 s | 30.68 s | 24.36 s | 5.95 s |
| App launch | 26.51 s | **8.48 s** | 47.31 s | 28.15 s | 37.11 s | 19.54 s |
| Navigation | 57.40 s | **25.79 s** | 81.40 s | 63.17 s | — | 48.91 s |
| Scroll to element | 46.17 s | **14.62 s** | 62.96 s | 58.75 s | 54.78 s | 34.36 s |
| Text input | 40.41 s | **20.35 s** | 110.58 s | 81.34 s | — | 39.75 s |
| Assertion burst | 26.91 s | **7.78 s** | 44.40 s | 30.62 s | 41.33 s | 21.35 s |
| Repeated taps | — | **38.38 s** | 108.32 s | 85.77 s | 103.45 s | 65.63 s |
| Whole suite (sum of medians) | — | **115.40 s** | 454.96 s | 347.80 s | — | 229.54 s |
| Peak host memory | 30.0 MB | **1.2 MB** | 591.4 MB | 142.6 MB | 32.8 MB | 70.6 MB |
| Host CPU time, whole suite | 18.58 s | **0.12 s** | 213.86 s | 38.56 s | 91.52 s | 2.89 s |
| Peak device agent memory | 121.7 MB | 133.8 MB | 157.5 MB | 290.8 MB | — | **87.0 MB** |
| Runs passed | 87% | **100%** | **100%** | 93% | 73% | **100%** |
| Run-to-run variation | 13.8% | 11.3% | 11.6% | **8.0%** | 14.5% | **7.9%** |
| app/animations | 238.25 s | **61.02 s** | — | 160.30 s | — | 219.36 s |
| app/appearance | 87.22 s | **29.31 s** | 435.05 s | 134.52 s | 102.59 s | — |
| app/cart | — | **41.60 s** | — | — | — | — |
| app/chat | 67.26 s | **34.19 s** | 330.36 s | — | — | 73.94 s |
| app/deep_links | 56.52 s | 32.20 s | 143.78 s | 47.73 s | 49.04 s | **26.85 s** |
| app/device | — | **79.61 s** | 723.13 s | 240.90 s | — | — |
| app/drag | — | **42.26 s** | — | 171.97 s | 143.42 s | 173.22 s |
| app/feed | — | **37.54 s** | — | — | — | 349.06 s |
| app/gestures | 61.74 s | **18.56 s** | 300.45 s | 95.11 s | — | 59.66 s |
| app/inputs | — | **102.58 s** | — | — | — | — |
| app/native | — | **30.77 s** | — | — | — | — |
| app/onboarding | — | **14.80 s** | — | 71.68 s | 99.63 s | 89.49 s |
| app/popups | — | **34.04 s** | 239.51 s | 92.04 s | 83.34 s | 60.99 s |
| app/profile | 205.08 s | **70.84 s** | — | 290.91 s | 181.10 s | 207.64 s |
| app/remember_me | — | **32.31 s** | 358.88 s | 185.33 s | 92.98 s | 101.97 s |
| app/scripting | — | **25.70 s** | 342.67 s | 98.11 s | 78.86 s | 58.66 s |
| app/selectors | — | **32.16 s** | 389.72 s | — | 105.76 s | 78.03 s |
| app/shop_checkout | — | **135.05 s** | — | — | — | — |
| app/sign_in | 67.82 s | **20.89 s** | 262.34 s | 100.06 s | 68.40 s | 81.10 s |
| app/timing | 94.95 s | **34.29 s** | 370.63 s | 132.43 s | 171.12 s | 92.04 s |
| app/Whole app suite (sum) | — | **909.74 s** | — | — | — | — |
| app/App flows passed | 38% | **99%** | 53% | 60% | 54% | 67% |
| tam/async_states | 28.64 s | **19.06 s** | 45.36 s | 29.74 s | 32.74 s | 23.73 s |
| tam/bottom_tabs | 15.88 s | **6.05 s** | 33.79 s | 23.42 s | 25.30 s | 14.43 s |
| tam/broken_accessibility | 27.10 s | **11.89 s** | 55.39 s | 34.25 s | 39.39 s | 27.46 s |
| tam/choice_controls | 23.77 s | **8.63 s** | 47.38 s | 29.90 s | 34.96 s | 20.39 s |
| tam/control_inventory | 26.71 s | **15.34 s** | 73.93 s | 33.31 s | 38.50 s | 29.86 s |
| tam/debounced_search | 22.76 s | **11.55 s** | 51.09 s | — | 32.01 s | 24.53 s |
| tam/error_recovery | 19.48 s | **8.58 s** | 36.93 s | 25.29 s | — | 17.99 s |
| tam/flattened_login | 28.33 s | **17.28 s** | 68.13 s | 40.36 s | 41.35 s | 34.23 s |
| tam/flattened_registration_form | 20.84 s | **13.73 s** | 56.66 s | 31.87 s | 30.16 s | 33.42 s |
| tam/gestures | 20.49 s | **8.94 s** | 43.17 s | 28.45 s | 32.86 s | — |
| tam/huge_virtualized_list | — | **40.23 s** | 265.76 s | 365.52 s | — | 171.38 s |
| tam/infinite_scroll_list | — | **23.39 s** | 102.53 s | 111.50 s | 79.90 s | 57.54 s |
| tam/login_form | 29.62 s | **19.17 s** | 73.70 s | 42.35 s | 42.93 s | 36.75 s |
| tam/modal_flow | 29.99 s | **12.09 s** | 58.06 s | 38.56 s | 42.50 s | — |
| tam/permission_prompt | 26.14 s | **14.69 s** | 47.12 s | 29.11 s | 33.03 s | — |
| tam/photo_picker | 20.19 s | **9.59 s** | 37.65 s | 26.79 s | — | 18.68 s |
| tam/product_catalog | 30.83 s | **11.17 s** | 57.46 s | 34.47 s | 41.94 s | 22.61 s |
| tam/sequential_onboarding | 49.02 s | **26.92 s** | 101.55 s | — | 56.65 s | 50.60 s |
| tam/sticky_chrome_target | 23.47 s | **7.09 s** | — | 32.68 s | 31.86 s | 19.23 s |
| tam/text_input_variations | 32.97 s | **23.92 s** | 85.96 s | 48.02 s | 47.78 s | 45.17 s |
| tam/vision_only | 15.33 s | **5.53 s** | 31.38 s | 22.22 s | 24.04 s | — |
| tam/webview_accessibility | 27.54 s | **15.25 s** | 53.12 s | — | 35.15 s | 28.71 s |
| tam/Whole app suite (sum) | — | **330.10 s** | — | — | — | — |
| tam/App flows passed | 91% | **100%** | 95% | 85% | 83% | 84% |

</details>

### iOS (macOS runner)

![iOS (macOS runner)](docs/bench/ios-macos-runner.svg)

bench (iOS 26.2 simulator); GitHub macos-15 runner (Apple Silicon, 3 vCPU, 7 GB).

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 12 | 2 | 54 |
| [speedrun](https://github.com/hubastard/speedrun) | 14 | 25 | 2 | 41 |
| [argent](https://github.com/software-mansion/argent) | 5 | 1 | 11 | 17 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 3 | 13 | 17 |
| [appium](https://github.com/appium/appium) | 1 | 2 | 9 | 12 |
| [e2e](https://github.com/tester-army/e2e) | 1 | 1 | 10 | 12 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 0 | 1 | 1 | 2 |

![iOS (macOS runner): every scenario](docs/bench/ios-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 48.7 MB | 4.9 MB | **2.2 MB** | 361.5 MB | 122.1 MB | 41.6 MB | 238.6 MB |
| CLI start (--version) | 15 ms | 4 ms | **3 ms** | 1575 ms | 560 ms | 42 ms | 50 ms |
| Invalid flow reported | 30 ms | 6 ms | **4 ms** | 2529 ms | — | — | 324 ms |
| One-command flow (connect + run) | 10.59 s | 1.58 s | **1.12 s** | 53.42 s | 19.27 s | 19.42 s | 1.86 s |
| App launch | 10.67 s | 8.23 s | **4.12 s** | 60.30 s | 20.66 s | 20.51 s | 9.39 s |
| Navigation | 27.25 s | **16.99 s** | **16.90 s** | 58.89 s | 24.45 s | 27.15 s | 19.26 s |
| Scroll to element | — | **11.85 s** | **11.90 s** | — | 22.86 s | 34.62 s | 20.50 s |
| Text input | **18.25 s** | — | 19.11 s | — | 26.12 s | 48.28 s | 38.18 s |
| Assertion burst | 12.61 s | 8.24 s | **5.23 s** | 62.60 s | 23.22 s | 37.46 s | 13.91 s |
| Repeated taps | 33.10 s | **19.49 s** | 26.70 s | 107.61 s | 56.49 s | 69.79 s | — |
| Whole suite (sum of medians) | — | — | **83.97 s** | — | 173.80 s | 237.81 s | — |
| Peak host memory | 26.9 MB | **13.8 MB** | **13.7 MB** | 212.9 MB | 199.7 MB | 136.8 MB | 87.3 MB |
| Host CPU time, whole suite | 15.39 s | 4.61 s | **0.98 s** | 126.89 s | 37.28 s | 13.12 s | 4.93 s |
| Runs passed | 83% | 83% | **100%** | 67% | **100%** | **100%** | 80% |
| Run-to-run variation | 11.3% | 13.6% | **7.1%** | 10.8% | 16.4% | 16.7% | 16.9% |
| app/animations | — | 71.40 s | **51.06 s** | — | — | 98.74 s | — |
| app/appearance | — | 17.00 s | **15.08 s** | 95.16 s | 82.69 s | 118.28 s | — |
| app/cart | — | 25.64 s | **24.81 s** | — | — | 62.12 s | 36.88 s |
| app/chat | — | 28.83 s | **26.00 s** | — | — | 45.90 s | — |
| app/deep_links | 62.27 s | 38.09 s | 32.52 s | 108.44 s | 67.77 s | 25.85 s | **17.77 s** |
| app/device | — | 56.76 s | **44.05 s** | 115.65 s | — | — | — |
| app/drag | — | **44.73 s** | **44.91 s** | — | 84.00 s | — | — |
| app/feed | — | 71.44 s | **61.16 s** | — | — | — | — |
| app/gestures | 22.99 s | **14.49 s** | **14.48 s** | 85.49 s | 92.77 s | 31.66 s | — |
| app/inputs | — | — | **82.33 s** | — | — | — | — |
| app/native | — | **26.28 s** | 28.30 s | — | 99.18 s | 124.25 s | — |
| app/onboarding | — | 17.78 s | **16.81 s** | — | — | 75.97 s | 25.81 s |
| app/popups | — | 35.46 s | **33.64 s** | 85.20 s | 108.65 s | 70.79 s | — |
| app/profile | — | **35.75 s** | 50.03 s | — | — | — | — |
| app/remember_me | 61.61 s | 29.30 s | **27.04 s** | 103.17 s | — | — | — |
| app/scripting | 41.74 s | 21.13 s | **17.22 s** | 80.87 s | 66.34 s | 27.83 s | — |
| app/selectors | — | 32.07 s | **25.02 s** | — | — | — | — |
| app/shop_checkout | — | **80.00 s** | 93.26 s | — | — | 183.04 s | — |
| app/sign_in | 24.46 s | **20.37 s** | 21.64 s | — | 60.28 s | — | — |
| app/timing | 30.64 s | **19.11 s** | 24.31 s | — | 73.47 s | 58.99 s | 32.07 s |
| app/Whole app suite (sum) | — | — | **733.66 s** | — | — | — | — |
| app/App flows passed | 30% | 95% | **97%** | 38% | 41% | 54% | 21% |
| tam/async_states | 31.41 s | 23.82 s | 19.39 s | 83.11 s | — | 41.70 s | **17.76 s** |
| tam/bottom_tabs | 31.82 s | 11.12 s | **8.70 s** | 73.86 s | 23.81 s | 31.75 s | **8.70 s** |
| tam/broken_accessibility | — | — | — | — | — | — | — |
| tam/choice_controls | 28.60 s | 8.99 s | 9.68 s | 56.97 s | 26.57 s | 24.90 s | **8.70 s** |
| tam/control_inventory | — | — | — | — | — | — | — |
| tam/debounced_search | 40.23 s | — | **13.43 s** | 120.98 s | — | 57.37 s | — |
| tam/error_recovery | 22.87 s | 11.32 s | **9.78 s** | 51.96 s | 17.38 s | 23.11 s | 17.62 s |
| tam/flattened_login | — | — | — | — | — | — | — |
| tam/flattened_registration_form | — | — | — | — | — | — | — |
| tam/gestures | — | 12.34 s | **9.70 s** | — | 20.95 s | — | — |
| tam/huge_virtualized_list | 160.97 s | **59.00 s** | 62.54 s | 285.26 s | 210.83 s | 177.97 s | 101.89 s |
| tam/infinite_scroll_list | 58.41 s | **15.73 s** | 17.03 s | 122.15 s | 71.45 s | 38.55 s | 25.72 s |
| tam/login_form | 37.62 s | — | **11.99 s** | — | — | — | — |
| tam/modal_flow | 42.84 s | 11.27 s | **9.98 s** | 94.01 s | 15.32 s | 35.36 s | — |
| tam/permission_prompt | — | 15.43 s | 12.56 s | 44.41 s | 14.31 s | — | **9.82 s** |
| tam/photo_picker | 37.72 s | 21.26 s | **14.58 s** | 77.46 s | — | 54.91 s | — |
| tam/product_catalog | 41.57 s | 14.18 s | **10.12 s** | 66.85 s | 12.69 s | 30.40 s | 10.51 s |
| tam/sequential_onboarding | 35.30 s | **22.32 s** | 33.17 s | — | — | 56.99 s | — |
| tam/sticky_chrome_target | 35.25 s | 12.36 s | **10.48 s** | 74.57 s | 30.82 s | 39.46 s | 16.06 s |
| tam/text_input_variations | 30.03 s | — | **14.58 s** | — | 19.91 s | 37.32 s | — |
| tam/vision_only | — | — | — | — | — | — | — |
| tam/webview_accessibility | — | — | — | — | — | — | — |
| tam/Whole app suite (sum) | — | — | — | — | — | — | — |
| tam/App flows passed | 64% | 60% | **73%** | 55% | 50% | 62% | 42% |

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
| [virtuoso](https://github.com/marksmayo/virtuoso) | 116 | 4 | 3 | 123 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 11 | 57 | 39 | 107 |
| [playwright](https://github.com/microsoft/playwright) | 10 | 41 | 29 | 80 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 10 | 20 | 50 | 80 |
| [e2e](https://github.com/tester-army/e2e) | 6 | 5 | 1 | 12 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 2 | 0 | 1 | 3 |
| [cypress](https://github.com/cypress-io/cypress) | 1 | 4 | 5 | 10 |

### Chrome (Linux runner)

![Chrome (Linux runner)](docs/bench/chrome-linux-runner.svg)

Chrome for Testing 157.0.8103.78, headless, 1280 x 800; GitHub Linux runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 39 | 1 | 1 | 41 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 4 | 2 | 23 | 29 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 2 | 31 | 3 | 36 |
| [playwright](https://github.com/microsoft/playwright) | 2 | 6 | 16 | 24 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 1 | 0 | 3 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 1 | 3 |

![Chrome (Linux runner): every scenario](docs/bench/chrome-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.8 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 73.6 MB |
| CLI start (--version) | **1 ms** | 1364 ms | 331 ms | 136 ms | 90 ms | 28 ms | 28 ms |
| Peak host memory | 239.9 MB | 296.7 MB | 231.8 MB | 249.2 MB | **70.5 MB** | 468.5 MB | 264.7 MB |
| Host CPU time, whole suite | 17.92 s | 566.79 s | 38.57 s | 33.83 s | **8.12 s** | 145.43 s | 102.25 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |
| Run-to-run variation | 6.5% | **1.3%** | 3.4% | 3.9% | 2.9% | 2.0% | 18.2% |
| aria_hidden_flow | **0.36 s** | 9.59 s | 0.83 s | 0.72 s | 0.84 s | 4.27 s | 2.39 s |
| async_states | **2.51 s** | — | 3.04 s | 2.63 s | 2.80 s | 5.98 s | 4.07 s |
| cart_totals | **0.35 s** | 9.50 s | 0.74 s | 0.73 s | 0.75 s | 3.55 s | 3.43 s |
| checkout_review | **0.47 s** | 7.45 s | 0.65 s | 0.60 s | 0.70 s | 3.42 s | 1.91 s |
| context_menu_trap | **0.42 s** | 18.49 s | 1.15 s | 1.07 s | 1.21 s | 5.99 s | 3.02 s |
| control_inventory | **1.16 s** | — | 2.18 s | 1.85 s | 2.41 s | 7.02 s | 3.82 s |
| date_picker | **0.44 s** | 18.19 s | 1.03 s | 0.84 s | 0.98 s | 4.65 s | 2.56 s |
| debounced_search | **1.77 s** | 11.02 s | 1.98 s | 1.84 s | 1.94 s | 5.01 s | 3.28 s |
| deferred_dialog | **1.23 s** | — | 1.97 s | 1.53 s | 1.78 s | — | 2.94 s |
| div_soup | **0.56 s** | 17.41 s | 0.87 s | 0.91 s | 0.82 s | 3.87 s | 2.17 s |
| filter_deep_link | **0.36 s** | 9.61 s | 1.05 s | 1.02 s | 1.03 s | 5.63 s | 2.65 s |
| gift_card_purchase | **0.54 s** | — | 1.04 s | 1.16 s | 1.06 s | 5.20 s | 2.62 s |
| iframe_form | **0.39 s** | — | 0.93 s | 0.79 s | 0.93 s | 4.37 s | 2.64 s |
| infinite_scroll | **2.55 s** | — | 3.34 s | 3.26 s | 3.33 s | 8.35 s | 6.87 s |
| kanban_board | **0.70 s** | 10.58 s | 1.14 s | 0.94 s | 1.65 s | 4.75 s | 2.65 s |
| login_form | **0.57 s** | 15.61 s | 0.72 s | 0.67 s | 0.78 s | 3.67 s | 2.01 s |
| lying_labels | **0.39 s** | 13.67 s | 1.08 s | 0.97 s | 1.10 s | 5.72 s | 2.92 s |
| native_dialogs | **0.49 s** | — | 0.96 s | 0.83 s | 0.88 s | 4.96 s | 4.01 s |
| new_tab_flow | **0.41 s** | — | 1.00 s | 0.93 s | 1.03 s | 4.78 s | 4.06 s |
| newsletter_signup | **0.51 s** | 13.51 s | 0.75 s | 0.70 s | 0.77 s | 4.01 s | 2.13 s |
| onboarding_wizard | **0.98 s** | — | 2.27 s | 1.85 s | 2.94 s | 9.17 s | 4.73 s |
| order_history | **0.50 s** | 10.49 s | 0.73 s | 0.64 s | 0.74 s | 3.56 s | 2.00 s |
| otp_auto_advance | **0.71 s** | — | 1.90 s | 1.28 s | 1.49 s | 6.11 s | 3.17 s |
| overlay_trap | **0.97 s** | 12.27 s | 1.77 s | 1.31 s | 1.35 s | 5.52 s | 3.03 s |
| playbook_cleanup | **0.41 s** | 17.45 s | 0.96 s | 0.93 s | 1.02 s | 4.86 s | 4.09 s |
| price_sorting | **0.42 s** | 6.95 s | 0.73 s | 0.62 s | 0.69 s | 3.89 s | 2.05 s |
| product_reviews | **0.29 s** | 6.70 s | 0.76 s | 0.68 s | 0.72 s | 3.68 s | 2.00 s |
| promo_storefront | **1.72 s** | 14.59 s | 2.04 s | 1.77 s | 1.99 s | 4.63 s | 3.04 s |
| saved_addresses | **0.36 s** | 11.61 s | 1.08 s | 1.01 s | 1.05 s | 5.78 s | 2.83 s |
| shadow_dom_form | **0.52 s** | — | 1.00 s | 0.90 s | 1.01 s | 5.15 s | 2.63 s |
| stale_dom | **0.38 s** | — | 0.92 s | 0.80 s | 0.89 s | 4.54 s | 3.91 s |
| sticky_chrome | **0.48 s** | — | 0.98 s | 0.87 s | 0.93 s | 5.29 s | 2.61 s |
| virtualized_table | **1.09 s** | — | 7.78 s | 14.35 s | 14.47 s | 62.16 s | 24.29 s |
| wishlist | **0.52 s** | 13.29 s | 0.78 s | 0.69 s | 0.75 s | 3.58 s | 3.53 s |
| Whole app suite (sum) | **25.52 s** | — | 50.16 s | 51.70 s | 56.84 s | — | 126.09 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |

</details>

### Chrome (Windows runner)

![Chrome (Windows runner)](docs/bench/chrome-windows-runner.svg)

Chrome for Testing 157.0.8103.78, headless, 1280 x 800; GitHub Windows runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 37 | 2 | 2 | 41 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 6 | 5 | 24 | 35 |
| [playwright](https://github.com/microsoft/playwright) | 5 | 29 | 4 | 38 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 2 | 1 | 5 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 1 | 14 | 17 |
| [cypress](https://github.com/cypress-io/cypress) | 1 | 1 | 2 | 4 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 0 | 0 | 1 | 1 |

![Chrome (Windows runner): every scenario](docs/bench/chrome-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.4 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 78.4 MB |
| CLI start (--version) | **6 ms** | 1249 ms | 292 ms | 151 ms | 108 ms | 38 ms | 36 ms |
| Peak host memory | **6.4 MB** | 267.5 MB | 95.6 MB | 58.6 MB | 50.8 MB | 36.4 MB | 38.0 MB |
| Host CPU time, whole suite | **44.86 s** | 767.67 s | 83.73 s | 82.62 s | 93.50 s | 697.44 s | 221.45 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **99%** | 97% | **100%** |
| Run-to-run variation | 14.1% | 6.8% | 10.2% | 11.3% | 10.4% | **3.1%** | 5.7% |
| aria_hidden_flow | **1.36 s** | 13.80 s | 1.46 s | 1.82 s | 1.90 s | 8.04 s | 2.99 s |
| async_states | **3.44 s** | — | 3.82 s | **3.45 s** | 3.87 s | 8.74 s | 4.47 s |
| cart_totals | **1.36 s** | 15.36 s | 1.73 s | 2.24 s | 2.19 s | 8.26 s | 2.85 s |
| checkout_review | **1.47 s** | 13.89 s | 1.90 s | 2.09 s | 2.08 s | 10.14 s | 3.68 s |
| context_menu_trap | **1.62 s** | 21.74 s | 1.91 s | 2.06 s | 2.18 s | 10.72 s | 4.00 s |
| control_inventory | **2.16 s** | — | 2.68 s | 2.25 s | 3.20 s | 9.37 s | 3.95 s |
| date_picker | **1.38 s** | 25.86 s | 1.62 s | 1.96 s | 1.96 s | 8.28 s | 3.17 s |
| debounced_search | 2.62 s | 13.23 s | **2.19 s** | 2.31 s | 2.95 s | 7.87 s | 3.66 s |
| deferred_dialog | 2.37 s | — | 2.41 s | **2.15 s** | 3.03 s | — | 3.77 s |
| div_soup | **1.53 s** | 28.13 s | 2.04 s | 2.07 s | 2.25 s | 10.62 s | 3.90 s |
| filter_deep_link | **1.53 s** | 12.15 s | 1.86 s | 1.96 s | 1.98 s | 10.30 s | 3.78 s |
| gift_card_purchase | **1.47 s** | — | 1.57 s | 1.95 s | 1.96 s | 7.90 s | 3.00 s |
| iframe_form | **1.38 s** | — | 1.62 s | 1.92 s | 1.94 s | 7.98 s | 3.17 s |
| infinite_scroll | 3.54 s | — | **3.33 s** | 3.85 s | 4.09 s | 9.47 s | 5.24 s |
| kanban_board | **1.40 s** | 14.70 s | 1.61 s | 1.89 s | 2.69 s | 8.50 s | 3.00 s |
| login_form | **1.58 s** | 25.40 s | 1.84 s | 2.01 s | 2.23 s | 10.46 s | 3.70 s |
| lying_labels | **1.51 s** | 19.71 s | 1.91 s | 2.03 s | 2.18 s | 10.43 s | 3.88 s |
| native_dialogs | **1.39 s** | — | 1.79 s | 1.84 s | 1.75 s | 7.91 s | 2.91 s |
| new_tab_flow | **1.57 s** | — | 1.61 s | 1.82 s | 1.88 s | 8.19 s | 3.13 s |
| newsletter_signup | **1.39 s** | 17.04 s | 1.49 s | 1.77 s | 1.77 s | 6.93 s | 2.50 s |
| onboarding_wizard | 3.04 s | — | **2.26 s** | **2.24 s** | 3.50 s | 11.60 s | 4.71 s |
| order_history | **1.55 s** | 15.81 s | 1.83 s | 2.05 s | 2.12 s | 10.27 s | 3.76 s |
| otp_auto_advance | **1.86 s** | — | 2.72 s | 2.13 s | 2.42 s | 10.74 s | 4.17 s |
| overlay_trap | **1.85 s** | 13.78 s | 1.92 s | 2.24 s | 2.61 s | 8.35 s | 3.40 s |
| playbook_cleanup | **1.36 s** | 23.02 s | 1.63 s | 1.78 s | 1.84 s | 8.09 s | 3.05 s |
| price_sorting | **1.30 s** | 9.81 s | 1.54 s | 1.75 s | 1.70 s | 6.99 s | 2.59 s |
| product_reviews | **1.67 s** | 10.62 s | 2.00 s | 2.28 s | 2.64 s | 8.76 s | 2.70 s |
| promo_storefront | **2.71 s** | 23.50 s | 2.86 s | **2.69 s** | 3.19 s | 11.43 s | 4.69 s |
| saved_addresses | **1.49 s** | 16.46 s | 1.83 s | 2.17 s | 2.13 s | 10.56 s | 3.94 s |
| shadow_dom_form | **1.40 s** | — | 1.65 s | 1.75 s | 2.04 s | 8.15 s | 3.02 s |
| stale_dom | **1.35 s** | — | 1.65 s | 1.82 s | 1.83 s | 7.94 s | 2.92 s |
| sticky_chrome | **1.36 s** | — | 1.50 s | 1.68 s | 1.70 s | 6.98 s | 2.54 s |
| virtualized_table | **4.61 s** | — | 8.07 s | 14.87 s | 15.57 s | 65.41 s | 24.46 s |
| wishlist | **1.60 s** | 22.13 s | 1.93 s | 2.14 s | 2.18 s | 10.37 s | 3.83 s |
| Whole app suite (sum) | **63.20 s** | — | 73.77 s | 85.04 s | 93.57 s | — | 140.53 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **99%** | 97% | **100%** |

</details>

### Chrome (macOS runner)

![Chrome (macOS runner)](docs/bench/chrome-macos-runner.svg)

Chrome for Testing 157.0.8103.78, headless, 1280 x 800; GitHub macOS runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 1 | 0 | 41 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 4 | 17 | 13 | 34 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 3 | 21 | 12 | 36 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 6 | 9 | 18 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 2 | 0 | 4 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 1 | 2 | 3 |

![Chrome (macOS runner): every scenario](docs/bench/chrome-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.3 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 66.6 MB |
| CLI start (--version) | **6 ms** | 1027 ms | 267 ms | 108 ms | 68 ms | 30 ms | 30 ms |
| Peak host memory | **211.0 MB** | 260.6 MB | **207.6 MB** | **217.3 MB** | **213.6 MB** | 452.5 MB | 244.7 MB |
| Host CPU time, whole suite | 64.50 s | 656.18 s | 108.89 s | 111.29 s | **61.26 s** | 331.24 s | 211.54 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |
| Run-to-run variation | 21.9% | **12.8%** | 18.4% | 18.8% | 17.7% | 17.4% | 16.2% |
| aria_hidden_flow | **2.14 s** | 26.38 s | 3.28 s | 2.87 s | 2.82 s | 13.56 s | 6.39 s |
| async_states | **3.31 s** | — | 3.89 s | 3.55 s | 3.54 s | 10.50 s | 5.73 s |
| cart_totals | **2.52 s** | 31.71 s | 6.22 s | 5.30 s | 3.82 s | 20.88 s | 7.50 s |
| checkout_review | **1.80 s** | 20.66 s | 3.73 s | 2.93 s | 2.67 s | 12.36 s | 4.64 s |
| context_menu_trap | **1.83 s** | 48.27 s | 4.05 s | 2.77 s | 2.48 s | 12.28 s | 5.33 s |
| control_inventory | **2.38 s** | — | 3.76 s | 2.94 s | 3.04 s | 8.61 s | 5.01 s |
| date_picker | **1.82 s** | 46.93 s | 3.11 s | 2.34 s | 2.47 s | 10.68 s | 5.26 s |
| debounced_search | **2.12 s** | 19.53 s | 2.91 s | 2.59 s | 2.82 s | 6.67 s | 4.59 s |
| deferred_dialog | **3.43 s** | — | 5.25 s | 5.79 s | 4.78 s | — | 9.45 s |
| div_soup | **2.35 s** | 48.92 s | 4.11 s | 4.08 s | 2.99 s | 13.69 s | 6.64 s |
| filter_deep_link | **1.24 s** | 19.15 s | 2.44 s | 2.16 s | 2.02 s | 8.96 s | 5.14 s |
| gift_card_purchase | **0.99 s** | — | 1.89 s | 1.97 s | 1.96 s | 7.17 s | 4.30 s |
| iframe_form | **1.23 s** | — | 2.61 s | 1.94 s | 2.27 s | 7.98 s | 4.19 s |
| infinite_scroll | **3.48 s** | — | 5.27 s | 5.08 s | 5.61 s | 8.54 s | 7.82 s |
| kanban_board | **2.85 s** | 31.37 s | 4.49 s | 4.48 s | 5.31 s | 14.90 s | 8.06 s |
| login_form | **1.31 s** | 43.20 s | 2.20 s | 2.11 s | 2.41 s | 7.61 s | 4.25 s |
| lying_labels | **1.54 s** | 25.19 s | 2.35 s | 1.74 s | 1.91 s | 8.40 s | 4.80 s |
| native_dialogs | **1.25 s** | — | 2.19 s | 2.29 s | 2.14 s | 8.01 s | 4.25 s |
| new_tab_flow | **1.47 s** | — | 2.79 s | 2.27 s | 2.75 s | 10.35 s | 5.41 s |
| newsletter_signup | **1.05 s** | 23.76 s | 1.91 s | 1.35 s | 1.42 s | 7.34 s | 4.22 s |
| onboarding_wizard | **4.07 s** | — | 6.65 s | 6.82 s | 8.49 s | 22.45 s | 15.55 s |
| order_history | **1.21 s** | 23.60 s | 2.31 s | 2.08 s | 2.27 s | 8.52 s | 5.32 s |
| otp_auto_advance | **1.35 s** | — | 2.33 s | 1.88 s | 1.85 s | 6.62 s | 4.20 s |
| overlay_trap | **1.95 s** | 22.44 s | 2.78 s | 2.51 s | 2.61 s | 9.95 s | 5.27 s |
| playbook_cleanup | **1.31 s** | 43.68 s | 1.92 s | 1.64 s | 1.64 s | 6.75 s | 4.59 s |
| price_sorting | **1.13 s** | 17.45 s | 2.40 s | 2.25 s | 2.22 s | 7.87 s | 4.15 s |
| product_reviews | **1.71 s** | 20.85 s | 3.82 s | 3.48 s | 2.88 s | 15.14 s | 8.09 s |
| promo_storefront | **2.59 s** | 43.86 s | 3.35 s | 2.66 s | 2.82 s | 7.42 s | 4.96 s |
| saved_addresses | **0.98 s** | 17.88 s | 1.89 s | 1.75 s | 1.49 s | 7.23 s | 4.73 s |
| shadow_dom_form | **1.09 s** | — | 1.75 s | 1.78 s | 1.92 s | 6.91 s | 3.56 s |
| stale_dom | **1.42 s** | — | 2.60 s | 2.37 s | 2.63 s | 9.39 s | 5.53 s |
| sticky_chrome | **0.90 s** | — | 1.48 s | 1.29 s | 1.26 s | 5.01 s | 2.81 s |
| virtualized_table | **9.62 s** | — | 34.25 s | 39.16 s | 40.67 s | 106.05 s | 93.10 s |
| wishlist | **0.97 s** | 41.82 s | 1.91 s | 1.74 s | 1.51 s | 6.33 s | 4.05 s |
| Whole app suite (sum) | **70.40 s** | — | 137.93 s | 131.94 s | 133.47 s | — | 278.92 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | **100%** | 97% | **100%** |

</details>
<!-- benchmark-web:end -->

### Web benchmark on phones

Two more fields run the same flows on a phone's screen. **Pixel 7 (emulated)**: desktop Chrome
emulating a Pixel 7 (412 × 839, touch, a phone's user agent), each tool its own way: virtuoso
`--emulate pixel-7`, Playwright's and Puppeteer's device emulation, chromedriver's mobile
emulation for Selenium, Cypress and e2e a viewport and user agent (neither emulates touch),
Maestro's web mode a window of that size (all it has). **Chrome on Android**: the real Chrome on
a Pixel 7 emulator (API 36, its Chrome 133, no on-screen keyboard as in the other fields) on
GitHub's Linux runners (`.github/workflows/web-android-bench.yml`): virtuoso `--device android`,
Playwright's Android support, Puppeteer over adb, Selenium and Appium through chromedriver's
Android mode, Maestro through Chrome the app (its web mode drives a desktop Chrome only).
Cypress drives no device, and e2e's engine needs browser contexts Chrome on Android cannot
make. The emulator crashes every few minutes on those runners, whatever its image or renderer,
so each flow runs a round at a time (every tool once, in turn) and a round the emulator did not
live through is run again, whole, on the emulator started afresh (up to five times; a round it
never lived through is left out). Android's "isn't responding" dialogs are off: on the slow
emulator they covered the page for Maestro, which reads the screen. A flow whose swipes go
between screen points has a phone copy (`bench/web-flows/pixel-7/`), and the Android field has
copies of its own (`bench/web-flows/android/`), each saying what it changes: Chrome on Android
shows a tool that reads its accessibility tree (Maestro) no `data-testid`, splits a text like
"$54.00" in two, names a glyph button only by its `aria-label`, and its prompt's field takes
typing once tapped; there an element is found by its text, its name or its row instead.

What Maestro still misses through Chrome the app is its own result, each for a reason the page
or Chrome gives it: content hidden from the accessibility tree (`aria_hidden_flow`), labels in
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
| [virtuoso](https://github.com/marksmayo/virtuoso) | 155 | 4 | 4 | 163 |
| [playwright](https://github.com/microsoft/playwright) | 12 | 46 | 51 | 109 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 11 | 99 | 36 | 146 |
| [e2e](https://github.com/tester-army/e2e) | 7 | 2 | 4 | 13 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 6 | 15 | 48 | 69 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 3 | 0 | 1 | 4 |
| [appium](https://github.com/appium/appium) | 2 | 1 | 1 | 4 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 6 | 3 | 9 |

### Chrome as a Pixel 7 (Linux runner)

![Chrome as a Pixel 7 (Linux runner)](docs/bench/chrome-as-a-pixel-7-linux-runner.svg)

Chrome for Testing 157.0.8103.78, headless, emulating a pixel-7; GitHub Linux runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 39 | 1 | 1 | 41 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 2 | 34 | 2 | 38 |
| [playwright](https://github.com/microsoft/playwright) | 2 | 4 | 26 | 32 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 2 | 9 | 13 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 0 | 1 | 3 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 2 | 1 | 3 |

![Chrome as a Pixel 7 (Linux runner): every scenario](docs/bench/chrome-as-a-pixel-7-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.8 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 73.6 MB |
| CLI start (--version) | **1 ms** | 1171 ms | 277 ms | 110 ms | 76 ms | 25 ms | 25 ms |
| Peak host memory | 240.1 MB | 298.3 MB | 230.7 MB | 249.5 MB | **70.4 MB** | 468.6 MB | 267.1 MB |
| Host CPU time, whole suite | 17.41 s | 574.01 s | 38.54 s | 34.18 s | **8.09 s** | 145.58 s | 101.72 s |
| Runs passed | **100%** | 56% | **100%** | **100%** | 96% | 98% | **100%** |
| Run-to-run variation | 6.9% | **1.1%** | 2.2% | 2.9% | 2.5% | 1.4% | 16.3% |
| aria_hidden_flow | **0.33 s** | 8.70 s | 0.71 s | 0.64 s | 0.79 s | 3.51 s | 1.94 s |
| async_states | **2.49 s** | — | 3.13 s | 2.71 s | 3.02 s | 6.22 s | 4.15 s |
| cart_totals | **0.34 s** | 11.86 s | 1.08 s | 1.03 s | 1.18 s | 5.41 s | 2.76 s |
| checkout_review | **0.49 s** | 8.02 s | 0.70 s | 0.62 s | 0.75 s | 3.76 s | 2.04 s |
| context_menu_trap | **0.43 s** | 18.46 s | 1.17 s | 1.09 s | 1.36 s | 5.79 s | 2.95 s |
| control_inventory | **1.02 s** | — | 2.17 s | 1.84 s | — | 6.86 s | 5.31 s |
| date_picker | **0.40 s** | 17.48 s | 0.90 s | 0.75 s | 1.22 s | 3.92 s | 2.22 s |
| debounced_search | **1.71 s** | 11.50 s | 2.03 s | 1.90 s | 2.09 s | 5.32 s | 4.90 s |
| deferred_dialog | **1.22 s** | — | 2.31 s | 1.82 s | 2.07 s | — | 5.03 s |
| div_soup | **0.57 s** | 17.94 s | 0.92 s | 0.93 s | 1.23 s | 4.20 s | 2.31 s |
| filter_deep_link | **0.40 s** | 9.45 s | 0.98 s | 0.94 s | 0.98 s | 5.38 s | 2.69 s |
| gift_card_purchase | **0.39 s** | — | 1.04 s | 0.98 s | 1.19 s | 5.14 s | 2.62 s |
| iframe_form | **0.36 s** | — | 0.80 s | 0.74 s | 1.10 s | 3.58 s | 2.21 s |
| infinite_scroll | **2.54 s** | — | 3.29 s | 3.18 s | 3.15 s | 8.18 s | 5.57 s |
| kanban_board | **0.67 s** | — | 0.88 s | 0.83 s | 2.19 s | 3.92 s | 2.11 s |
| login_form | **0.61 s** | 16.62 s | 0.77 s | 0.71 s | 0.94 s | 4.09 s | 3.61 s |
| lying_labels | **0.41 s** | 13.36 s | 1.07 s | 0.95 s | 1.19 s | 5.68 s | 4.27 s |
| native_dialogs | **0.36 s** | — | 0.97 s | 0.84 s | 0.93 s | 5.04 s | 2.62 s |
| new_tab_flow | **0.40 s** | — | 0.75 s | 0.68 s | 0.93 s | 3.65 s | 2.15 s |
| newsletter_signup | **0.53 s** | 13.63 s | 0.79 s | 0.78 s | 0.87 s | 4.02 s | 2.32 s |
| onboarding_wizard | **1.04 s** | — | 2.21 s | 1.80 s | 4.93 s | 8.82 s | 4.63 s |
| order_history | **0.51 s** | 11.13 s | 0.80 s | 0.64 s | 0.91 s | 3.90 s | 2.18 s |
| otp_auto_advance | **0.73 s** | — | 1.84 s | 1.28 s | 1.47 s | 5.92 s | 3.11 s |
| overlay_trap | **0.84 s** | 12.32 s | 1.80 s | 1.33 s | 1.50 s | 5.55 s | 2.97 s |
| playbook_cleanup | **0.41 s** | 15.86 s | 0.77 s | 0.73 s | 0.99 s | 3.77 s | 2.09 s |
| price_sorting | **0.43 s** | 7.08 s | 0.73 s | 0.66 s | 0.71 s | 3.92 s | 2.05 s |
| product_reviews | **0.31 s** | 8.61 s | 1.01 s | 0.92 s | 0.95 s | 5.42 s | 2.63 s |
| promo_storefront | **1.67 s** | 15.51 s | 2.08 s | 1.79 s | 2.04 s | 4.96 s | 3.15 s |
| saved_addresses | **0.39 s** | 11.29 s | 1.05 s | 0.98 s | 1.13 s | 5.48 s | 2.86 s |
| shadow_dom_form | **0.40 s** | — | 1.00 s | 0.92 s | 1.28 s | 5.12 s | 2.62 s |
| stale_dom | **0.34 s** | — | 0.77 s | 0.68 s | 0.95 s | 3.77 s | 2.12 s |
| sticky_chrome | **0.55 s** | — | 0.97 s | 0.89 s | 1.00 s | 5.29 s | 2.76 s |
| virtualized_table | **1.21 s** | — | 11.13 s | 11.01 s | 4.01 s | 56.96 s | 24.32 s |
| wishlist | **0.54 s** | 14.08 s | 0.83 s | 0.74 s | 1.03 s | 3.98 s | 2.15 s |
| Whole app suite (sum) | **25.04 s** | — | 53.43 s | 48.34 s | — | — | 123.42 s |
| App flows passed | **100%** | 56% | **100%** | **100%** | 96% | 98% | **100%** |

</details>

### Chrome as a Pixel 7 (Windows runner)

![Chrome as a Pixel 7 (Windows runner)](docs/bench/chrome-as-a-pixel-7-windows-runner.svg)

Chrome for Testing 157.0.8103.78, headless, emulating a pixel-7; GitHub Windows runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 36 | 2 | 2 | 40 |
| [playwright](https://github.com/microsoft/playwright) | 7 | 29 | 3 | 39 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 5 | 5 | 26 | 36 |
| [e2e](https://github.com/tester-army/e2e) | 3 | 1 | 1 | 5 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 0 | 3 | 4 | 7 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 3 | 1 | 4 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 0 | 0 | 1 | 1 |

![Chrome as a Pixel 7 (Windows runner): every scenario](docs/bench/chrome-as-a-pixel-7-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.4 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 78.4 MB |
| CLI start (--version) | **5 ms** | 1351 ms | 314 ms | 168 ms | 118 ms | 39 ms | 38 ms |
| Peak host memory | **6.4 MB** | 264.5 MB | 96.8 MB | 58.5 MB | 50.6 MB | 36.4 MB | 38.0 MB |
| Host CPU time, whole suite | **41.91 s** | 730.61 s | 80.66 s | 80.95 s | 82.16 s | 687.30 s | 210.59 s |
| Runs passed | **100%** | 59% | **100%** | **100%** | 78% | 97% | **100%** |
| Run-to-run variation | 18.1% | 7.4% | 19.8% | 13.9% | 17.1% | 6.5% | **3.3%** |
| aria_hidden_flow | **1.28 s** | 13.65 s | 1.60 s | 1.72 s | 1.85 s | 7.85 s | 2.80 s |
| async_states | **3.28 s** | — | **3.34 s** | **3.28 s** | 3.90 s | 8.80 s | 4.38 s |
| cart_totals | **1.53 s** | 14.85 s | 1.56 s | 2.05 s | 2.12 s | 8.59 s | 3.04 s |
| checkout_review | **1.61 s** | 13.86 s | 1.91 s | 2.15 s | — | 10.05 s | 3.49 s |
| context_menu_trap | **1.87 s** | 22.48 s | 1.97 s | 2.12 s | 2.33 s | 8.82 s | 3.08 s |
| control_inventory | **2.34 s** | — | **2.32 s** | 3.36 s | — | 9.91 s | 4.20 s |
| date_picker | **1.48 s** | 25.28 s | 1.66 s | 1.84 s | 2.32 s | 8.40 s | 3.08 s |
| debounced_search | 2.53 s | 12.81 s | **2.20 s** | 2.42 s | 2.89 s | 7.75 s | 3.51 s |
| deferred_dialog | **2.25 s** | — | 2.70 s | 2.53 s | 3.00 s | — | 4.03 s |
| div_soup | **1.78 s** | 23.33 s | 2.05 s | 2.23 s | — | 10.40 s | 3.86 s |
| filter_deep_link | **1.45 s** | 11.50 s | 1.59 s | 1.79 s | 1.90 s | 8.27 s | 2.89 s |
| gift_card_purchase | **1.53 s** | — | 1.62 s | 1.98 s | 2.10 s | 8.45 s | 3.21 s |
| iframe_form | **1.35 s** | — | 1.56 s | 1.78 s | 2.10 s | 7.94 s | 3.04 s |
| infinite_scroll | 3.38 s | — | **3.25 s** | 3.71 s | 3.79 s | 9.45 s | 5.02 s |
| kanban_board | **1.62 s** | 14.75 s | 1.65 s | 1.89 s | 3.36 s | 8.72 s | 3.32 s |
| login_form | **1.67 s** | 25.46 s | 2.03 s | 2.25 s | — | 10.16 s | 3.71 s |
| lying_labels | 2.18 s | 17.94 s | **1.93 s** | 2.23 s | 2.04 s | 8.81 s | 3.01 s |
| native_dialogs | **1.46 s** | — | 1.64 s | 1.82 s | 1.85 s | 8.37 s | 3.11 s |
| new_tab_flow | **1.38 s** | — | 1.61 s | 1.81 s | 2.02 s | 8.00 s | 2.92 s |
| newsletter_signup | **1.24 s** | 16.78 s | 1.36 s | 1.57 s | 1.60 s | 6.39 s | 2.21 s |
| onboarding_wizard | **2.02 s** | — | 2.41 s | 2.33 s | 5.75 s | 11.97 s | 4.96 s |
| order_history | **1.68 s** | 19.11 s | 1.97 s | 2.28 s | — | 10.12 s | 3.68 s |
| otp_auto_advance | **1.92 s** | — | 2.06 s | 2.00 s | 2.49 s | 8.89 s | 3.44 s |
| overlay_trap | 2.76 s | 15.99 s | 2.64 s | **2.47 s** | 2.64 s | 9.19 s | 3.56 s |
| playbook_cleanup | **1.39 s** | 23.02 s | 1.63 s | 1.73 s | 2.00 s | 8.11 s | 2.88 s |
| price_sorting | **1.20 s** | 8.75 s | 1.35 s | 1.66 s | 1.46 s | 6.23 s | 2.17 s |
| product_reviews | **1.49 s** | 10.65 s | 1.62 s | 1.93 s | 1.80 s | 8.47 s | 2.92 s |
| promo_storefront | 2.84 s | 20.83 s | 2.89 s | **2.71 s** | — | 11.24 s | 4.63 s |
| saved_addresses | **1.44 s** | 15.12 s | 1.69 s | 1.88 s | 1.97 s | 8.42 s | 3.01 s |
| shadow_dom_form | **1.44 s** | — | 1.75 s | 1.96 s | 2.37 s | 8.73 s | 3.08 s |
| stale_dom | **1.31 s** | — | 1.51 s | 1.83 s | 2.06 s | 7.91 s | 2.88 s |
| sticky_chrome | **1.25 s** | — | 1.40 s | 1.60 s | 1.61 s | 6.28 s | 2.23 s |
| virtualized_table | **4.45 s** | — | 11.21 s | 11.64 s | — | 70.84 s | 24.63 s |
| wishlist | **1.67 s** | 22.38 s | 2.06 s | 2.25 s | — | 10.31 s | 3.72 s |
| Whole app suite (sum) | **64.06 s** | — | 75.73 s | 82.81 s | — | — | 135.70 s |
| App flows passed | **100%** | 59% | **100%** | **100%** | 78% | 97% | **100%** |

</details>

### Chrome as a Pixel 7 (macOS runner)

![Chrome as a Pixel 7 (macOS runner)](docs/bench/chrome-as-a-pixel-7-macos-runner.svg)

Chrome for Testing 157.0.8103.78, headless, emulating a pixel-7; GitHub macOS runner.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 1 | 0 | 41 |
| [playwright](https://github.com/microsoft/playwright) | 3 | 11 | 17 | 31 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 2 | 9 | 13 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 1 | 2 | 5 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 1 | 28 | 4 | 33 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 0 | 0 | 1 |
| [cypress](https://github.com/cypress-io/cypress) | 0 | 1 | 1 | 2 |

![Chrome as a Pixel 7 (macOS runner): every scenario](docs/bench/chrome-as-a-pixel-7-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | virtuoso | maestro | playwright | puppeteer | selenium | cypress | e2e |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | **2.3 MB** | 361.5 MB | 14.0 MB | 27.2 MB | 25.7 MB | 18.3 MB | 66.6 MB |
| CLI start (--version) | **4 ms** | 935 ms | 264 ms | 105 ms | 74 ms | 33 ms | 34 ms |
| Peak host memory | **212.2 MB** | 271.9 MB | **209.1 MB** | 219.2 MB | **213.9 MB** | 451.9 MB | 245.4 MB |
| Host CPU time, whole suite | 67.03 s | 619.83 s | 100.34 s | 106.85 s | **46.03 s** | 303.79 s | 192.58 s |
| Runs passed | **100%** | 56% | **100%** | 97% | 64% | 97% | **100%** |
| Run-to-run variation | 16.4% | **7.6%** | 14.7% | 16.5% | 13.7% | 14.1% | 11.7% |
| aria_hidden_flow | **2.04 s** | 24.43 s | 3.05 s | 2.93 s | 2.87 s | 12.58 s | 5.45 s |
| async_states | **2.98 s** | — | 3.97 s | 3.49 s | — | 7.88 s | 5.42 s |
| cart_totals | **2.40 s** | 27.35 s | 3.73 s | 3.17 s | — | 14.60 s | 7.40 s |
| checkout_review | **1.33 s** | 18.29 s | 2.61 s | 1.96 s | 2.24 s | 8.58 s | 4.54 s |
| context_menu_trap | **0.87 s** | 45.78 s | 1.24 s | 1.11 s | 1.79 s | 5.00 s | 2.41 s |
| control_inventory | **2.70 s** | — | 5.06 s | — | 7.00 s | 14.41 s | 7.49 s |
| date_picker | **1.58 s** | 45.31 s | 3.23 s | 2.01 s | 3.15 s | 12.20 s | 5.62 s |
| debounced_search | **2.25 s** | 19.16 s | 2.57 s | 2.56 s | — | 6.46 s | 4.33 s |
| deferred_dialog | **2.44 s** | — | 4.15 s | 3.71 s | — | — | 6.86 s |
| div_soup | **1.58 s** | 44.92 s | 3.36 s | 3.18 s | 3.04 s | 8.90 s | 4.78 s |
| filter_deep_link | **0.66 s** | 11.37 s | 0.96 s | 0.79 s | 0.85 s | 3.14 s | 1.69 s |
| gift_card_purchase | **1.59 s** | — | 2.97 s | 2.94 s | 3.74 s | 8.79 s | 5.04 s |
| iframe_form | **1.84 s** | — | 2.84 s | 2.82 s | 3.10 s | 10.28 s | 6.95 s |
| infinite_scroll | **3.43 s** | — | 5.48 s | 5.51 s | — | 10.35 s | 8.57 s |
| kanban_board | **1.97 s** | — | 2.65 s | 2.43 s | — | 9.25 s | 5.56 s |
| login_form | **1.14 s** | 43.03 s | 1.84 s | 1.70 s | 2.13 s | 6.53 s | 3.79 s |
| lying_labels | **0.85 s** | 18.48 s | 1.15 s | 0.93 s | 1.47 s | 3.80 s | 2.17 s |
| native_dialogs | **1.35 s** | — | 2.29 s | 2.12 s | 2.17 s | 8.63 s | 4.74 s |
| new_tab_flow | **1.29 s** | — | 2.27 s | 1.89 s | 2.47 s | 8.46 s | 4.84 s |
| newsletter_signup | **1.27 s** | 23.98 s | 1.86 s | 1.59 s | — | 7.40 s | 4.68 s |
| onboarding_wizard | **4.02 s** | — | 6.48 s | 6.55 s | — | 19.01 s | 11.91 s |
| order_history | **1.01 s** | 20.73 s | 1.77 s | 1.55 s | 1.98 s | 6.93 s | 3.61 s |
| otp_auto_advance | **1.06 s** | — | 1.92 s | 1.50 s | 1.61 s | 4.12 s | 2.60 s |
| overlay_trap | **1.90 s** | 24.47 s | 2.81 s | 2.32 s | 3.02 s | 9.40 s | 5.19 s |
| playbook_cleanup | **1.90 s** | 46.88 s | 2.72 s | **1.90 s** | 2.64 s | 9.35 s | 5.64 s |
| price_sorting | **1.28 s** | 16.29 s | 2.34 s | 2.05 s | — | 9.67 s | 4.75 s |
| product_reviews | **1.55 s** | 19.74 s | 3.03 s | 3.80 s | — | 12.40 s | 5.91 s |
| promo_storefront | **2.09 s** | 43.48 s | 3.26 s | 2.76 s | 3.05 s | 8.37 s | 4.99 s |
| saved_addresses | **0.64 s** | 14.40 s | 1.06 s | 1.14 s | 1.38 s | 3.42 s | 2.16 s |
| shadow_dom_form | **1.37 s** | — | 2.29 s | 2.00 s | 2.52 s | 8.38 s | 5.00 s |
| stale_dom | **1.16 s** | — | 2.22 s | 1.93 s | 2.48 s | 7.14 s | 4.53 s |
| sticky_chrome | **1.25 s** | — | 2.41 s | 1.63 s | — | 5.96 s | 4.14 s |
| virtualized_table | **5.77 s** | — | 38.26 s | 43.89 s | — | 49.28 s | 80.18 s |
| wishlist | **1.22 s** | 43.28 s | 2.13 s | 1.96 s | 2.48 s | 7.06 s | 3.95 s |
| Whole app suite (sum) | **61.81 s** | — | 129.94 s | — | — | — | 246.91 s |
| App flows passed | **100%** | 56% | **100%** | 97% | 64% | 97% | **100%** |

</details>

### Chrome on a Pixel 7 emulator (Linux runner)

![Chrome on a Pixel 7 emulator (Linux runner)](docs/bench/chrome-on-a-pixel-7-emulator-linux-runner.svg)

Chrome on a Pixel 7 emulator (API 36, Google APIs); GitHub ubuntu-latest runner (4 vCPU, 16 GB), Android emulator on KVM.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 40 | 0 | 1 | 41 |
| [puppeteer](https://github.com/puppeteer/puppeteer) | 3 | 32 | 4 | 39 |
| [selenium](https://github.com/SeleniumHQ/selenium) | 2 | 8 | 26 | 36 |
| [appium](https://github.com/appium/appium) | 2 | 1 | 1 | 4 |
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
| Run-to-run variation | 4.8% | **3.0%** | 9.2% | 8.2% | 3.7% | 3.9% |
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

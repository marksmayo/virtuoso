# virtuoso

[![CI](https://github.com/marksmayo/virtuoso/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/marksmayo/virtuoso/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-Android%20%7C%20iOS-lightgrey.svg)](#benchmark)
[![Maestro compatible](https://img.shields.io/badge/flows-Maestro%20compatible-6f42c1.svg)](https://github.com/mobile-dev-inc/maestro)

A fast, Maestro-compatible end-to-end test runner for Android and iOS, written in Rust: one binary
with its device agent built in, waiting for the app instead of the clock, reading the screen
without screenshots. [speedrun](https://github.com/hubastard/speedrun) set the challenge: it
is the bar to beat, and one of the tools the benchmark below compares against.

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

Android devices and emulators, and iOS simulators (`--platform ios`, on a Mac with Xcode; not
real iPhones yet). Runs on Windows (natively), Linux and macOS. On iOS, what a simulator does not
have is not supported, as in Maestro: airplane mode, the Back key, the volume keys; the camera and
notification permissions are the app's to ask for (simctl cannot set them).

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
- `crates/virtuoso` — the host: flow parser (`flow`), selector matching (`select`), the runner
  (`runner`, against a `Device` trait), the Android device (`android`, `android_device`), the
  iOS simulator (`ios`),
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
Each of the 186 performance events and 55 features is a medal event. A performance metric is an event of its own on each platform and host (Android (Linux runner), Android (Windows runner), iOS (macOS runner)), so a tool that wins on one and not another shows it. Written by `virtuoso-bench readme`, from GitHub Actions runs.

**Overall**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 193 | 21 | 5 | 219 |
| [speedrun](https://github.com/hubastard/speedrun) | 52 | 70 | 6 | 128 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 38 | 26 | 54 | 118 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 25 | 6 | 9 | 40 |
| [argent](https://github.com/software-mansion/argent) | 10 | 28 | 39 | 77 |
| [e2e](https://github.com/tester-army/e2e) | 6 | 7 | 44 | 57 |
| [appium](https://github.com/appium/appium) | 2 | 5 | 16 | 23 |

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 149 | 21 | 5 | 175 |
| [speedrun](https://github.com/hubastard/speedrun) | 30 | 69 | 4 | 103 |
| [argent](https://github.com/software-mansion/argent) | 10 | 28 | 39 | 77 |
| [e2e](https://github.com/tester-army/e2e) | 6 | 7 | 44 | 57 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 3 | 25 | 51 | 79 |
| [appium](https://github.com/appium/appium) | 2 | 5 | 16 | 23 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 2 | 3 | 5 | 10 |

### Android (Linux runner)

![Android (Linux runner)](docs/bench/android-linux-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub ubuntu-latest runner (4 vCPU, 16 GB), Android emulator on KVM.

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 53 | 6 | 2 | 61 |
| [speedrun](https://github.com/hubastard/speedrun) | 14 | 42 | 2 | 58 |
| [argent](https://github.com/software-mansion/argent) | 4 | 0 | 14 | 18 |
| [e2e](https://github.com/tester-army/e2e) | 3 | 1 | 22 | 26 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 1 | 20 | 22 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 1 | 0 | 2 |
| [appium](https://github.com/appium/appium) | 1 | 0 | 2 | 3 |

![Android (Linux runner): every scenario](docs/bench/android-linux-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 49.5 MB | 3.7 MB | **2.9 MB** | 361.5 MB | 147.7 MB | 42.8 MB | 243.4 MB |
| CLI start (--version) | 7 ms | **1 ms** | **1 ms** | 2255 ms | 598 ms | 40 ms | 42 ms |
| Invalid flow reported | 10 ms | **1 ms** | 1 ms | 2300 ms | — | — | 203 ms |
| One-command flow (connect + run) | 2.80 s | 1.30 s | **0.69 s** | 13.25 s | 12.91 s | 9.12 s | 1.53 s |
| App launch | 5.53 s | 1.99 s | **1.76 s** | 15.28 s | 10.76 s | 10.59 s | 5.42 s |
| Navigation | 23.19 s | **8.69 s** | 8.97 s | 38.43 s | 28.03 s | 29.86 s | 21.39 s |
| Scroll to element | 20.28 s | 5.65 s | **5.26 s** | 27.98 s | 31.48 s | 20.85 s | 15.48 s |
| Text input | 15.15 s | 9.60 s | **8.98 s** | 50.55 s | 38.87 s | 23.17 s | 16.57 s |
| Assertion burst | 7.81 s | 3.42 s | **2.65 s** | 18.99 s | 12.17 s | 14.02 s | 7.47 s |
| Repeated taps | 32.37 s | **12.44 s** | 13.03 s | 50.40 s | 35.04 s | 39.21 s | 29.78 s |
| Whole suite (sum of medians) | 104.34 s | 41.78 s | **40.65 s** | 201.63 s | 156.34 s | 137.71 s | 96.10 s |
| Peak host memory | 26.8 MB | 5.8 MB | **4.5 MB** | 624.3 MB | 177.3 MB | 155.0 MB | 95.2 MB |
| Host CPU time, whole suite | 1.98 s | 1.13 s | **0.03 s** | 114.60 s | 13.49 s | 10.13 s | 1.42 s |
| Peak device agent memory | 132.6 MB | 148.0 MB | 144.3 MB | 158.5 MB | 284.8 MB | — | **90.2 MB** |
| Runs passed | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** |
| Run-to-run variation | 5.2% | 5.3% | 5.2% | 3.2% | 5.7% | **1.9%** | 4.7% |
| app/animations | 193.03 s | 107.05 s | **56.69 s** | — | 123.90 s | 102.88 s | 196.94 s |
| app/appearance | 50.62 s | 15.59 s | **13.49 s** | 313.93 s | 86.96 s | 45.35 s | 58.57 s |
| app/cart | — | 21.07 s | **17.43 s** | — | — | 52.19 s | — |
| app/chat | 54.32 s | 24.99 s | **24.46 s** | 259.38 s | 69.12 s | 43.60 s | 66.08 s |
| app/deep_links | 54.01 s | 29.28 s | 30.26 s | 165.78 s | 36.87 s | 29.89 s | **23.54 s** |
| app/device | — | 42.93 s | **41.10 s** | 531.48 s | 136.45 s | — | — |
| app/drag | — | 31.21 s | **28.99 s** | — | 162.01 s | 95.73 s | 145.73 s |
| app/feed | — | 38.91 s | **27.25 s** | — | — | 127.99 s | 342.52 s |
| app/gestures | 31.14 s | 11.34 s | **10.28 s** | 216.24 s | 52.00 s | 30.95 s | 45.93 s |
| app/inputs | — | — | **58.02 s** | — | — | — | — |
| app/native | — | 19.50 s | **18.89 s** | — | — | — | — |
| app/onboarding | — | 9.75 s | **9.52 s** | — | 47.65 s | 48.69 s | 70.16 s |
| app/popups | — | 33.79 s | **31.60 s** | 234.51 s | 78.02 s | 59.95 s | 61.88 s |
| app/profile | 135.05 s | **47.04 s** | **47.99 s** | 519.79 s | 226.84 s | 81.92 s | 165.32 s |
| app/remember_me | — | **23.30 s** | **22.84 s** | 397.55 s | 153.85 s | 54.78 s | 94.36 s |
| app/scripting | — | 17.65 s | **16.53 s** | 257.48 s | 63.53 s | 35.48 s | 50.05 s |
| app/selectors | — | 26.63 s | **18.86 s** | 238.78 s | 60.34 s | 37.56 s | 49.29 s |
| app/shop_checkout | — | 73.97 s | **70.96 s** | — | — | 100.70 s | — |
| app/sign_in | 52.50 s | 15.97 s | **13.97 s** | 297.44 s | 86.72 s | 40.16 s | 61.72 s |
| app/timing | 58.60 s | 20.41 s | **19.69 s** | 278.48 s | 74.67 s | 47.04 s | 73.21 s |
| app/Whole app suite (sum) | — | — | **578.83 s** | — | — | — | — |
| app/App flows passed | 40% | 91% | **100%** | 60% | 77% | 83% | 75% |
| tam/async_states | 24.38 s | 18.92 s | 17.91 s | 39.70 s | 23.15 s | 25.92 s | **17.16 s** |
| tam/bottom_tabs | 9.16 s | 3.32 s | **3.08 s** | 20.32 s | 13.02 s | 13.47 s | 7.92 s |
| tam/broken_accessibility | 10.12 s | 5.40 s | **5.07 s** | 27.18 s | 17.09 s | 15.63 s | 12.74 s |
| tam/choice_controls | 13.35 s | **5.27 s** | **5.20 s** | 33.94 s | 18.17 s | 18.15 s | 12.43 s |
| tam/control_inventory | 22.68 s | 14.84 s | **13.87 s** | 66.92 s | — | 22.53 s | 24.96 s |
| tam/debounced_search | 12.95 s | 9.78 s | **8.99 s** | 40.15 s | 21.60 s | 18.76 s | 17.61 s |
| tam/error_recovery | 13.71 s | 8.08 s | **7.67 s** | 32.97 s | 20.66 s | 21.07 s | 14.14 s |
| tam/flattened_login | 13.02 s | **10.06 s** | **9.89 s** | 40.93 s | 20.41 s | 17.33 s | 17.75 s |
| tam/flattened_registration_form | 15.23 s | **11.88 s** | 13.01 s | 50.37 s | 25.52 s | 19.31 s | 23.43 s |
| tam/gestures | 10.68 s | 5.71 s | **5.10 s** | 27.77 s | 16.44 s | 16.57 s | — |
| tam/huge_virtualized_list | — | 23.52 s | **19.01 s** | 229.97 s | 193.75 s | 109.34 s | 146.04 s |
| tam/infinite_scroll_list | — | 9.69 s | **9.17 s** | 62.06 s | 48.16 s | 32.00 s | 38.15 s |
| tam/login_form | 14.22 s | **10.30 s** | 10.52 s | 46.30 s | 22.83 s | 18.10 s | 19.46 s |
| tam/modal_flow | 11.85 s | 5.08 s | **4.85 s** | 30.09 s | 15.94 s | 16.73 s | — |
| tam/permission_prompt | 15.80 s | 11.74 s | **11.39 s** | 30.72 s | — | 17.45 s | — |
| tam/photo_picker | 13.08 s | **6.27 s** | **6.24 s** | 27.22 s | 17.21 s | 17.91 s | 12.39 s |
| tam/product_catalog | 10.12 s | **4.27 s** | **4.27 s** | 30.28 s | 16.65 s | 16.61 s | 10.97 s |
| tam/sequential_onboarding | 21.52 s | 13.78 s | **12.76 s** | 60.42 s | 36.99 s | 18.70 s | 27.23 s |
| tam/sticky_chrome_target | 18.37 s | 5.07 s | **3.93 s** | 30.70 s | 19.87 s | 16.96 s | 13.57 s |
| tam/text_input_variations | 11.67 s | 9.93 s | **9.54 s** | 41.39 s | 19.84 s | 14.75 s | 18.32 s |
| tam/vision_only | 9.88 s | 4.53 s | **4.31 s** | 25.93 s | 15.64 s | 16.69 s | — |
| tam/webview_accessibility | 17.16 s | 10.06 s | **9.49 s** | 39.59 s | 23.05 s | 19.73 s | 16.88 s |
| tam/Whole app suite (sum) | — | 207.49 s | **195.27 s** | 1034.92 s | — | 503.73 s | — |
| tam/App flows passed | 91% | **99%** | **100%** | 96% | 90% | **100%** | 85% |

</details>

### Android (Windows runner)

![Android (Windows runner)](docs/bench/android-windows-runner.svg)

emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, software GPU (SwiftShader); GitHub windows-latest runner (4 vCPU, 16 GB), Android emulator on WHPX.

Not measured: speedrun, which runs on Linux and macOS only, so here in WSL: under WSL 1 it finds no flows, and WSL 2 cannot reach the emulator's adb-forwarded ports on GitHub's Windows runners (mirrored networking is unsupported there).

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 59 | 1 | 1 | 61 |
| [argent](https://github.com/software-mansion/argent) | 4 | 25 | 14 | 43 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 21 | 19 | 41 |
| [e2e](https://github.com/tester-army/e2e) | 1 | 6 | 14 | 21 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 1 | 2 | 1 | 4 |
| [appium](https://github.com/appium/appium) | 0 | 3 | 8 | 11 |

![Android (Windows runner): every scenario](docs/bench/android-windows-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|
| Install size | 59.0 MB | **3.0 MB** | 361.5 MB | 129.8 MB | 41.4 MB | 238.3 MB |
| CLI start (--version) | 17 ms | **6 ms** | 2571 ms | 968 ms | 67 ms | 75 ms |
| Invalid flow reported | 48 ms | **5 ms** | 3729 ms | — | — | 378 ms |
| One-command flow (connect + run) | 7.48 s | **1.99 s** | 24.18 s | 23.84 s | 17.93 s | 4.54 s |
| App launch | 13.14 s | **4.68 s** | 27.55 s | 19.63 s | 22.92 s | 11.85 s |
| Navigation | 42.13 s | **18.95 s** | 64.70 s | 48.45 s | 55.33 s | 39.38 s |
| Scroll to element | 34.60 s | **11.32 s** | 47.55 s | 46.28 s | 40.81 s | 26.61 s |
| Text input | 30.17 s | **16.09 s** | 80.12 s | 64.49 s | 49.27 s | 31.00 s |
| Assertion burst | 18.47 s | **6.40 s** | 34.56 s | 23.90 s | 31.06 s | 16.30 s |
| Repeated taps | 55.19 s | **26.80 s** | 81.70 s | 62.87 s | 72.87 s | 50.95 s |
| Whole suite (sum of medians) | 193.70 s | **84.25 s** | 336.19 s | 265.61 s | 272.26 s | 176.08 s |
| Peak host memory | 30.1 MB | **1.6 MB** | 594.5 MB | 145.5 MB | 36.3 MB | 71.2 MB |
| Host CPU time, whole suite | 12.92 s | **0.05 s** | 161.28 s | 28.14 s | 68.42 s | 2.06 s |
| Peak device agent memory | 126.4 MB | 144.2 MB | 158.1 MB | 289.4 MB | — | **89.6 MB** |
| Runs passed | **100%** | **100%** | **100%** | 97% | **100%** | **100%** |
| Run-to-run variation | 8.9% | 9.6% | 8.3% | 5.3% | 8.2% | **5.0%** |
| app/animations | 238.73 s | **62.83 s** | — | 162.16 s | 206.85 s | 221.29 s |
| app/appearance | 80.24 s | **28.39 s** | 432.60 s | 132.67 s | 98.34 s | — |
| app/cart | — | **26.10 s** | — | — | 86.31 s | — |
| app/chat | — | **44.24 s** | — | — | 101.22 s | 90.63 s |
| app/deep_links | 53.91 s | 30.75 s | 132.78 s | 40.62 s | 40.65 s | **23.82 s** |
| app/device | — | **81.26 s** | 757.32 s | — | — | — |
| app/drag | — | **72.85 s** | — | 278.34 s | — | 270.87 s |
| app/feed | — | **52.19 s** | — | — | 309.17 s | — |
| app/gestures | 61.23 s | **21.25 s** | 280.11 s | 90.67 s | 80.75 s | 60.56 s |
| app/inputs | — | **91.38 s** | — | — | — | — |
| app/native | — | **42.64 s** | — | — | — | — |
| app/onboarding | — | **16.21 s** | — | 73.31 s | 104.02 s | 89.00 s |
| app/popups | — | **41.65 s** | 350.47 s | 125.35 s | 107.51 s | 80.01 s |
| app/profile | 135.76 s | **46.95 s** | 448.59 s | 193.78 s | 120.59 s | 144.81 s |
| app/remember_me | — | **44.49 s** | 453.38 s | — | 119.52 s | 131.20 s |
| app/scripting | — | **23.99 s** | 351.68 s | 92.61 s | 73.57 s | 60.30 s |
| app/selectors | — | **23.03 s** | 253.73 s | 76.45 s | 76.96 s | 59.07 s |
| app/shop_checkout | — | **142.30 s** | — | — | — | — |
| app/sign_in | 117.59 s | **34.02 s** | 429.18 s | 163.81 s | — | 126.80 s |
| app/timing | 86.53 s | **31.32 s** | 334.21 s | 121.85 s | 103.29 s | 89.73 s |
| app/Whole app suite (sum) | — | **957.83 s** | — | — | — | — |
| app/App flows passed | 36% | **97%** | 55% | 64% | 70% | 64% |
| tam/async_states | 38.65 s | **24.01 s** | 62.78 s | 38.03 s | 47.23 s | 27.37 s |
| tam/bottom_tabs | 28.66 s | **11.45 s** | 54.24 s | 34.69 s | 41.09 s | 21.79 s |
| tam/broken_accessibility | 27.54 s | **12.11 s** | 57.28 s | 35.42 s | 39.70 s | 27.69 s |
| tam/choice_controls | 32.63 s | **15.29 s** | 62.31 s | 36.14 s | 44.08 s | 23.70 s |
| tam/control_inventory | 26.98 s | **16.34 s** | 73.13 s | 32.83 s | 40.13 s | 30.40 s |
| tam/debounced_search | 21.83 s | **11.64 s** | 50.78 s | 31.70 s | 31.71 s | 24.50 s |
| tam/error_recovery | 34.29 s | **15.23 s** | 63.00 s | 39.54 s | 47.46 s | 27.63 s |
| tam/flattened_login | 23.57 s | **14.23 s** | 55.54 s | 33.28 s | 32.89 s | 28.24 s |
| tam/flattened_registration_form | 35.40 s | **25.68 s** | 88.41 s | — | 51.86 s | 50.19 s |
| tam/gestures | 21.09 s | **10.06 s** | 47.54 s | — | 36.24 s | — |
| tam/huge_virtualized_list | — | **32.28 s** | 234.88 s | 306.77 s | 171.79 s | 157.00 s |
| tam/infinite_scroll_list | — | **26.06 s** | 115.11 s | 116.24 s | 90.00 s | 58.45 s |
| tam/login_form | 25.09 s | **14.76 s** | 61.70 s | — | 34.67 s | 30.29 s |
| tam/modal_flow | 23.73 s | **9.99 s** | 44.59 s | 27.78 s | 31.85 s | — |
| tam/permission_prompt | 34.70 s | **18.49 s** | 61.32 s | 38.95 s | 46.80 s | — |
| tam/photo_picker | 38.60 s | **19.50 s** | 64.56 s | 44.90 s | 51.42 s | 30.22 s |
| tam/product_catalog | 27.83 s | **11.51 s** | 55.62 s | 36.23 s | 43.05 s | 22.42 s |
| tam/sequential_onboarding | 47.19 s | **27.31 s** | 102.88 s | — | — | 50.21 s |
| tam/sticky_chrome_target | 32.24 s | **11.88 s** | — | 40.83 s | 45.87 s | 23.57 s |
| tam/text_input_variations | 26.24 s | **16.96 s** | 66.54 s | 36.08 s | 35.62 s | 33.66 s |
| tam/vision_only | 26.25 s | **10.94 s** | 56.13 s | 37.88 s | 40.35 s | — |
| tam/webview_accessibility | 44.92 s | **21.05 s** | 68.67 s | — | 48.46 s | 34.65 s |
| tam/Whole app suite (sum) | — | **376.77 s** | — | — | — | — |
| tam/App flows passed | 89% | **100%** | 95% | 79% | 88% | 81% |

</details>

### iOS (macOS runner)

![iOS (macOS runner)](docs/bench/ios-macos-runner.svg)

bench (iOS 26.2 simulator); GitHub macos-15 runner (Apple Silicon, 3 vCPU, 7 GB).

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 37 | 14 | 2 | 53 |
| [speedrun](https://github.com/hubastard/speedrun) | 16 | 27 | 2 | 45 |
| [argent](https://github.com/software-mansion/argent) | 2 | 3 | 11 | 16 |
| [e2e](https://github.com/tester-army/e2e) | 2 | 0 | 8 | 10 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 1 | 3 | 12 | 16 |
| [appium](https://github.com/appium/appium) | 1 | 2 | 6 | 9 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 0 | 0 | 4 | 4 |

![iOS (macOS runner): every scenario](docs/bench/ios-macos-runner-scenarios.svg)

<details><summary>Every metric's scores</summary>

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 48.7 MB | 4.9 MB | **2.2 MB** | 361.5 MB | 122.1 MB | 41.6 MB | 238.6 MB |
| CLI start (--version) | 15 ms | 4 ms | **3 ms** | 1332 ms | 675 ms | 53 ms | 55 ms |
| Invalid flow reported | 31 ms | 6 ms | **5 ms** | 3715 ms | — | — | 416 ms |
| One-command flow (connect + run) | 8.44 s | 1.64 s | **1.00 s** | 50.42 s | 23.12 s | 22.81 s | 2.70 s |
| App launch | 11.23 s | 8.73 s | **5.39 s** | 57.81 s | 15.72 s | 21.20 s | 8.82 s |
| Navigation | 32.96 s | **17.37 s** | 19.76 s | 78.16 s | 32.52 s | 48.67 s | 22.52 s |
| Scroll to element | — | 13.75 s | **12.39 s** | — | 29.38 s | 37.99 s | 21.76 s |
| Text input | 23.89 s | — | **18.94 s** | — | 21.46 s | 33.78 s | 35.10 s |
| Assertion burst | 24.68 s | 11.86 s | **8.18 s** | 97.67 s | 34.42 s | 45.40 s | 18.94 s |
| Repeated taps | 35.98 s | **24.93 s** | 26.93 s | 107.75 s | 49.19 s | 59.62 s | 39.40 s |
| Whole suite (sum of medians) | — | — | **91.60 s** | — | 182.69 s | 246.66 s | 146.56 s |
| Peak host memory | 26.7 MB | **13.8 MB** | **13.7 MB** | 221.3 MB | 201.8 MB | 137.0 MB | 87.2 MB |
| Host CPU time, whole suite | 18.54 s | 5.06 s | 2.26 s | 132.06 s | 37.56 s | 13.08 s | **2.16 s** |
| Peak device agent memory | — | — | — | — | — | — | — |
| Runs passed | 83% | 83% | **100%** | 67% | **100%** | **100%** | 93% |
| Run-to-run variation | **7.6%** | 12.2% | 11.2% | 11.5% | 18.6% | 15.4% | 20.7% |
| app/animations | — | 69.43 s | **50.57 s** | — | — | 104.61 s | — |
| app/appearance | — | **17.61 s** | 23.90 s | 103.69 s | 107.60 s | 140.45 s | — |
| app/cart | — | **30.97 s** | 33.78 s | — | — | 79.29 s | — |
| app/chat | — | 30.19 s | **28.57 s** | — | — | 96.43 s | — |
| app/deep_links | 48.13 s | 33.17 s | 32.62 s | 86.29 s | 56.03 s | **28.51 s** | — |
| app/device | — | 50.99 s | **38.68 s** | 119.48 s | — | — | — |
| app/drag | — | 53.27 s | **48.81 s** | — | 103.50 s | — | — |
| app/feed | — | 76.95 s | **65.09 s** | — | 179.96 s | — | — |
| app/gestures | 29.46 s | **15.85 s** | **16.06 s** | 80.25 s | 63.46 s | 32.76 s | 19.27 s |
| app/inputs | — | — | **98.50 s** | — | — | — | — |
| app/native | — | **31.21 s** | 32.34 s | — | 110.88 s | — | — |
| app/onboarding | — | 21.13 s | **18.17 s** | — | — | 79.07 s | 22.42 s |
| app/popups | — | 33.33 s | **32.47 s** | 83.70 s | 93.06 s | 87.67 s | — |
| app/profile | — | **37.83 s** | 51.21 s | — | — | — | — |
| app/remember_me | 41.54 s | 28.13 s | **21.84 s** | 92.73 s | — | — | — |
| app/scripting | 26.92 s | **16.47 s** | 18.52 s | 78.91 s | 53.26 s | 32.22 s | — |
| app/selectors | — | 34.68 s | **27.76 s** | — | — | — | — |
| app/shop_checkout | — | **100.34 s** | 103.91 s | — | — | 246.62 s | — |
| app/sign_in | 34.41 s | 25.37 s | **23.21 s** | — | 61.98 s | — | — |
| app/timing | 42.16 s | **18.07 s** | 20.95 s | — | 79.09 s | 42.81 s | — |
| app/Whole app suite (sum) | — | — | **786.96 s** | — | — | — | — |
| app/App flows passed | 29% | 94% | **99%** | 36% | 40% | 51% | 20% |
| tam/async_states | 39.97 s | 20.72 s | **19.31 s** | 91.40 s | — | 39.72 s | 25.72 s |
| tam/bottom_tabs | 27.57 s | 12.14 s | **7.95 s** | 69.28 s | 25.60 s | 27.67 s | 14.18 s |
| tam/broken_accessibility | — | — | — | — | — | — | — |
| tam/choice_controls | 32.86 s | **8.68 s** | 10.17 s | 60.11 s | 16.69 s | 22.27 s | 9.10 s |
| tam/control_inventory | — | — | — | — | — | — | — |
| tam/debounced_search | 29.01 s | 13.90 s | **12.16 s** | 80.48 s | — | 32.02 s | — |
| tam/error_recovery | 26.00 s | **10.63 s** | **10.47 s** | 79.30 s | 19.03 s | 25.49 s | **10.50 s** |
| tam/flattened_login | — | — | — | — | — | — | — |
| tam/flattened_registration_form | — | — | — | — | — | — | — |
| tam/gestures | — | 10.91 s | **10.10 s** | — | 17.45 s | — | — |
| tam/huge_virtualized_list | 171.49 s | 75.99 s | **66.28 s** | 301.82 s | 229.01 s | — | — |
| tam/infinite_scroll_list | 58.02 s | 17.78 s | **16.87 s** | 110.01 s | 51.87 s | 67.65 s | 37.01 s |
| tam/login_form | 33.05 s | — | **17.26 s** | — | — | — | — |
| tam/modal_flow | 17.88 s | **7.74 s** | 8.48 s | 47.86 s | 27.39 s | 29.42 s | — |
| tam/permission_prompt | — | 16.06 s | **15.55 s** | 86.98 s | 28.03 s | — | — |
| tam/photo_picker | — | — | — | — | — | — | — |
| tam/product_catalog | 26.59 s | 9.81 s | **9.01 s** | 64.56 s | 17.72 s | 22.34 s | 11.48 s |
| tam/sequential_onboarding | 38.70 s | **25.07 s** | 46.99 s | — | — | 83.42 s | — |
| tam/sticky_chrome_target | 23.78 s | **10.15 s** | 10.80 s | 70.90 s | 36.53 s | 28.19 s | 13.56 s |
| tam/text_input_variations | 54.06 s | — | **15.91 s** | — | 31.48 s | 41.98 s | — |
| tam/vision_only | — | — | — | — | — | — | — |
| tam/webview_accessibility | — | — | — | — | — | — | — |
| tam/Whole app suite (sum) | — | — | — | — | — | — | — |
| tam/App flows passed | 59% | 56% | **68%** | 50% | 50% | 52% | 31% |

</details>
<!-- benchmark:end -->

## License

MIT, see [LICENSE](LICENSE). The faker word lists in `crates/virtuoso/src/datafaker` come from
Datafaker and keep its Apache 2.0 license.

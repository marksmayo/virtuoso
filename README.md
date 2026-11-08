# virtuoso

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

`report` also rewrites the results below (between the `benchmark` markers) from the same data.

### Latest results

<!-- benchmark:start -->
Each of the 62 performance metrics and 56 features is a medal event. Device: emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, host GPU. Written by `virtuoso-bench report`, which also writes the full HTML report.

**Overall**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 100 | 5 | 0 | 105 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 39 | 0 | 40 | 79 |
| [speedrun](https://github.com/hubastard/speedrun) | 30 | 49 | 6 | 85 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 27 | 2 | 5 | 34 |
| [argent](https://github.com/software-mansion/argent) | 3 | 1 | 11 | 15 |
| [e2e](https://github.com/tester-army/e2e) | 3 | 0 | 5 | 8 |
| [appium](https://github.com/appium/appium) | 3 | 0 | 4 | 7 |

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| [virtuoso](https://github.com/marksmayo/virtuoso) | 57 | 5 | 0 | 62 |
| [speedrun](https://github.com/hubastard/speedrun) | 6 | 49 | 4 | 59 |
| [argent](https://github.com/software-mansion/argent) | 3 | 1 | 11 | 15 |
| [e2e](https://github.com/tester-army/e2e) | 3 | 0 | 5 | 8 |
| [appium](https://github.com/appium/appium) | 3 | 0 | 4 | 7 |
| [maestro-runner](https://github.com/devicelab-dev/maestro-runner) | 2 | 0 | 37 | 39 |
| [maestro](https://github.com/mobile-dev-inc/maestro) | 2 | 0 | 1 | 3 |

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium | e2e | argent |
|---|---:|---:|---:|---:|---:|---:|---:|
| Install size | 49.3 MB | 3.7 MB | **2.3 MB** | 361.5 MB | 192.3 MB | 41.4 MB | 238.2 MB |
| CLI start (--version) | 17 ms | 4 ms | **2 ms** | 1925 ms | 719 ms | 38 ms | 42 ms |
| Invalid flow reported | 18 ms | **6 ms** | 8 ms | 1991 ms | — | — | 169 ms |
| One-command flow (connect + run) | 3.30 s | 1.93 s | **0.57 s** | 9.86 s | 11.31 s | 27.02 s | 4.69 s |
| App launch | 5.13 s | 2.48 s | **1.12 s** | 19.54 s | 7.89 s | 32.25 s | 6.94 s |
| Navigation | 10.26 s | 5.10 s | **3.80 s** | 33.43 s | 16.21 s | 49.90 s | 17.02 s |
| Scroll to element | 9.77 s | 4.43 s | **2.46 s** | 26.50 s | 23.09 s | 47.77 s | 15.88 s |
| Text input | 8.67 s | 7.22 s | **3.10 s** | 38.26 s | 29.52 s | 50.55 s | 18.66 s |
| Assertion burst | 5.94 s | 2.07 s | **1.26 s** | 16.58 s | 6.33 s | 47.87 s | 10.32 s |
| Repeated taps | 12.53 s | 5.65 s | **5.27 s** | 40.80 s | 22.75 s | 70.95 s | 32.76 s |
| Whole suite (sum of medians) | 52.30 s | 26.96 s | **17.01 s** | 175.12 s | 105.79 s | 299.30 s | 101.57 s |
| Peak host memory | 22.4 MB | 5.8 MB | **1.7 MB** | 614.2 MB | 202.3 MB | 55.0 MB | 81.5 MB |
| Host CPU time, whole suite | 9.25 s | 2.18 s | **0.03 s** | 181.50 s | 19.34 s | 226.66 s | 1.11 s |
| Peak device agent memory | 149.9 MB | 149.6 MB | 146.1 MB | 182.7 MB | 330.1 MB | — | **123.5 MB** |
| Runs passed | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** | **100%** |
| Run-to-run variation | 10.0% | 12.4% | 4.3% | 5.8% | 10.7% | **2.0%** | 4.6% |
| animations | 132.63 s | 120.73 s | **33.34 s** | 717.83 s | — | 290.64 s | 191.01 s |
| appearance | 42.52 s | 17.73 s | **8.98 s** | 348.52 s | — | 101.43 s | 57.06 s |
| cart | — | 27.36 s | **16.17 s** | — | — | 127.73 s | 63.92 s |
| chat | — | 31.77 s | **15.02 s** | 314.93 s | — | 96.56 s | 51.51 s |
| deep_links | 52.51 s | 29.60 s | **23.75 s** | 158.13 s | — | 66.66 s | **23.51 s** |
| device | — | 23.38 s | **20.53 s** | 114.12 s | — | 147.94 s | 121.92 s |
| drag | — | 15.85 s | **13.59 s** | — | — | 218.79 s | 184.12 s |
| feed | 98.49 s | 29.36 s | **18.37 s** | — | — | 192.27 s | 347.42 s |
| gestures | 16.80 s | 8.06 s | **6.12 s** | 74.55 s | — | 90.02 s | 48.15 s |
| inputs | — | — | **16.16 s** | — | — | — | — |
| native | 44.17 s | **15.45 s** | **15.30 s** | — | — | — | — |
| onboarding | — | 6.19 s | **4.40 s** | — | — | 47.61 s | 61.26 s |
| popups | — | 29.88 s | **26.74 s** | 92.78 s | — | 64.31 s | 36.17 s |
| profile | — | 23.33 s | **20.58 s** | — | — | — | 90.94 s |
| remember_me | — | 17.16 s | **7.89 s** | 260.02 s | — | 50.50 s | 50.26 s |
| scripting | — | 14.06 s | **10.74 s** | 125.75 s | — | 35.00 s | 23.17 s |
| selectors | — | 25.36 s | **14.46 s** | 152.09 s | — | 122.92 s | 39.64 s |
| shop_checkout | — | 64.30 s | **46.69 s** | — | — | 171.72 s | 170.82 s |
| sign_in | 27.41 s | 11.97 s | **9.71 s** | 146.17 s | — | 91.33 s | 83.68 s |
| timing | 41.06 s | 15.79 s | **11.61 s** | 138.22 s | — | 56.06 s | 53.68 s |
| Whole app suite (sum) | — | — | **340.14 s** | — | — | — | — |
| App flows passed | 43% | 95% | **100%** | 60% | — | 84% | 88% |
| async_states | 24.98 s | 22.44 s | **20.16 s** | 40.20 s | 23.98 s | 48.38 s | 29.55 s |
| bottom_tabs | 10.05 s | 8.64 s | **3.45 s** | 35.92 s | 13.54 s | 43.23 s | 27.31 s |
| broken_accessibility | 8.58 s | 5.14 s | **3.58 s** | 29.84 s | 13.59 s | 32.10 s | 35.62 s |
| choice_controls | 17.83 s | 8.33 s | **5.70 s** | 56.37 s | 21.27 s | 59.64 s | 36.23 s |
| control_inventory | 15.76 s | 9.50 s | **8.21 s** | 52.75 s | 19.72 s | 44.24 s | 40.02 s |
| debounced_search | 8.91 s | **5.60 s** | 6.69 s | 29.04 s | 14.57 s | 26.31 s | 24.39 s |
| error_recovery | 11.07 s | 6.89 s | **6.44 s** | 26.43 s | 15.27 s | 29.08 s | 23.46 s |
| flattened_login | 6.51 s | 3.56 s | **2.62 s** | 24.84 s | 10.93 s | 23.22 s | 36.07 s |
| flattened_registration_form | 9.34 s | 4.88 s | **4.19 s** | 33.92 s | 14.48 s | 27.85 s | 56.47 s |
| gestures | 10.86 s | 5.49 s | **4.54 s** | 22.90 s | 11.90 s | 26.14 s | 23.80 s |
| huge_virtualized_list | — | 16.06 s | **11.78 s** | 172.60 s | 108.31 s | 107.30 s | 159.79 s |
| infinite_scroll_list | 32.39 s | 7.48 s | **5.93 s** | 52.00 s | 29.96 s | 39.10 s | 52.18 s |
| login_form | 8.43 s | 4.46 s | **3.45 s** | 32.50 s | 13.71 s | 24.85 s | 39.32 s |
| modal_flow | 6.80 s | 2.83 s | **2.06 s** | 22.56 s | 8.78 s | 24.17 s | 29.77 s |
| permission_prompt | 12.66 s | 9.76 s | **9.08 s** | 24.48 s | **8.99 s** | 22.31 s | 25.20 s |
| photo_picker | 6.42 s | 3.33 s | **2.87 s** | 19.46 s | 9.50 s | 21.14 s | 25.03 s |
| product_catalog | 7.16 s | 2.91 s | **2.65 s** | 23.53 s | 9.63 s | 24.79 s | 27.67 s |
| sequential_onboarding | 14.16 s | 6.93 s | **4.86 s** | 44.57 s | 21.25 s | 30.85 s | 68.04 s |
| sticky_chrome_target | 9.68 s | 3.04 s | **2.44 s** | 24.70 s | 14.65 s | 25.80 s | 34.26 s |
| text_input_variations | 8.62 s | 3.89 s | **3.22 s** | 31.32 s | 12.82 s | 26.49 s | 76.52 s |
| vision_only | 5.75 s | 2.69 s | **2.02 s** | 16.33 s | 7.79 s | 20.58 s | 53.20 s |
| webview_accessibility | 8.42 s | **3.81 s** | 4.47 s | 23.40 s | 11.43 s | 22.79 s | 44.02 s |
| Whole app suite (sum) | — | 147.64 s | **120.41 s** | 839.64 s | 416.08 s | 750.35 s | 967.91 s |
| App flows passed | **99%** | **100%** | **100%** | **100%** | **99%** | **100%** | 93% |
<!-- benchmark:end -->

## License

MIT, see [LICENSE](LICENSE). The faker word lists in `crates/virtuoso/src/datafaker` come from
Datafaker and keep its Apache 2.0 license.

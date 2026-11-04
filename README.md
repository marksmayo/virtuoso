# virtuoso

A fast, Maestro-compatible end-to-end test runner for Android, written in Rust. It is a
ground-up rewrite of the ideas in [speedrun](https://github.com/hubastard/speedrun): one
binary with its device agent built in, waiting for the app instead of the clock, reading the
screen without screenshots.

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

Android only: no iOS, web or cloud yet. Runs on Windows (natively) and Linux.

## How it works

- `agent/android` — a small Java agent (no Gradle: `javac` + `d8`, built by `build.rs` and
  embedded in the binary). It runs as the shell user through `app_process`, reads every
  window's accessibility tree through a hidden `UiAutomation`, injects input straight into the
  input manager, runs device commands without an adb round trip, and tells when the UI has
  settled (accessibility events going quiet, a main-thread idle probe, and a stable-tree
  check for screens that never stop animating). It speaks length-prefixed JSON over one TCP
  connection through `adb forward`, after a per-run token.
- `crates/virtuoso` — the host: flow parser (`flow`), selector matching (`select`), the runner
  (`runner`, against a `Device` trait), the Android device (`android`, `android_device`),
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

## Tests

Every feature was built test first (red, then green). Most tests need no device: the runner
runs against a scripted `FakeDevice` (`crates/virtuoso/src/testing.rs`), the parser and
selectors against literals. Tests that need a real device or emulator pass trivially unless
`VIRTUOSO_DEVICE` names one:

```sh
cargo test --workspace                                            # no device needed
VIRTUOSO_DEVICE=emulator-5554 cargo test -p virtuoso -- --test-threads=1   # also on a device
```

`VIRTUOSO_DEBUG=1` traces every agent call with its time.

## Benchmark

`virtuoso-bench` runs the same flows (`bench/flows`, against Android's Settings app) with each
tool, resets the device before every run, and writes a results file; `virtuoso-bench report`
turns it into a self-contained HTML page with the medal tables. See
[docs/benchmark.md](docs/benchmark.md) for the metrics and how to run it.

```sh
cargo build --release -p virtuoso -p virtuoso-bench
target/release/virtuoso-bench run --iterations 5 --out bench/results/results.json
target/release/virtuoso-bench report bench/results/results.json --out report/index.html
```

`report` also rewrites the results below (between the `benchmark` markers) from the same data.

### Latest results

<!-- benchmark:start -->
Each of the 38 performance metrics and 56 features is a medal event. Device: emulator-5554: sdk_gphone64_x86_64 Android 16 (API 36), x86_64 emulator, host GPU. Written by `virtuoso-bench report`, which also writes the full HTML report.

**Overall**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| virtuoso | 80 | 1 | 0 | 81 |
| maestro-runner | 38 | 1 | 25 | 64 |
| speedrun | 27 | 32 | 2 | 61 |
| maestro | 26 | 3 | 11 | 40 |
| appium | 1 | 0 | 0 | 1 |

**Performance**

| Tool | 🥇 | 🥈 | 🥉 | Total |
|---|---:|---:|---:|---:|
| virtuoso | 37 | 1 | 0 | 38 |
| speedrun | 3 | 32 | 0 | 35 |
| maestro-runner | 1 | 1 | 22 | 24 |
| maestro | 1 | 1 | 7 | 9 |
| appium | 1 | 0 | 0 | 1 |

| Metric | maestro-runner | speedrun | virtuoso | maestro | appium |
|---|---:|---:|---:|---:|---:|
| Install size | 49.3 MB | 3.7 MB | **2.3 MB** | 361.5 MB | 192.3 MB |
| CLI start (--version) | 17 ms | 4 ms | **2 ms** | 1925 ms | 719 ms |
| Invalid flow reported | 18 ms | **6 ms** | 8 ms | 1991 ms | — |
| One-command flow (connect + run) | 3.30 s | 1.93 s | **0.57 s** | 9.86 s | 11.31 s |
| App launch | 5.13 s | 2.48 s | **1.12 s** | 19.54 s | 7.89 s |
| Navigation | 10.26 s | 5.10 s | **3.80 s** | 33.43 s | 16.21 s |
| Scroll to element | 9.77 s | 4.43 s | **2.46 s** | 26.50 s | 23.09 s |
| Text input | 8.67 s | 7.22 s | **3.10 s** | 38.26 s | 29.52 s |
| Assertion burst | 5.94 s | 2.07 s | **1.26 s** | 16.58 s | 6.33 s |
| Repeated taps | 12.53 s | 5.65 s | **5.27 s** | 40.80 s | 22.75 s |
| Whole suite (sum of medians) | 52.30 s | 26.96 s | **17.01 s** | 175.12 s | 105.79 s |
| Peak host memory | 22.4 MB | 5.8 MB | **1.7 MB** | 614.2 MB | 202.3 MB |
| Host CPU time, whole suite | 9.25 s | 2.18 s | **0.03 s** | 181.50 s | 19.34 s |
| Peak device agent memory | 149.9 MB | 149.6 MB | **146.1 MB** | 182.7 MB | 330.1 MB |
| Runs passed | **100%** | **100%** | **100%** | **100%** | **100%** |
| Run-to-run variation | 10.0% | 12.4% | **4.3%** | 5.8% | 10.7% |
| animations | 132.63 s | 120.73 s | **33.34 s** | 717.83 s | — |
| appearance | 42.52 s | 17.73 s | **8.98 s** | 348.52 s | — |
| cart | — | 27.36 s | **16.17 s** | — | — |
| chat | — | 31.77 s | **15.02 s** | 314.93 s | — |
| deep_links | 52.51 s | 29.60 s | **23.75 s** | 158.13 s | — |
| device | — | 23.38 s | **20.53 s** | 114.12 s | — |
| drag | — | 15.85 s | **13.59 s** | — | — |
| feed | 98.49 s | 29.36 s | **18.37 s** | — | — |
| gestures | 16.80 s | 8.06 s | **6.12 s** | 74.55 s | — |
| inputs | — | — | **16.16 s** | — | — |
| native | 44.17 s | **15.45 s** | **15.30 s** | — | — |
| onboarding | — | 6.19 s | **4.40 s** | — | — |
| popups | — | 29.88 s | **26.74 s** | 92.78 s | — |
| profile | — | 23.33 s | **20.58 s** | — | — |
| remember_me | — | 17.16 s | **7.89 s** | 260.02 s | — |
| scripting | — | 14.06 s | **10.74 s** | 125.75 s | — |
| selectors | — | 25.36 s | **14.46 s** | 152.09 s | — |
| shop_checkout | — | 64.30 s | **46.69 s** | — | — |
| sign_in | 27.41 s | 11.97 s | **9.71 s** | 146.17 s | — |
| timing | 41.06 s | 15.79 s | **11.61 s** | 138.22 s | — |
| Whole app suite (sum) | — | — | **340.14 s** | — | — |
| App flows passed | 43% | 95% | **100%** | 60% | — |
<!-- benchmark:end -->

## License

Apache 2.0.

# The benchmark

`virtuoso-bench` compares virtuoso, maestro-runner, speedrun, Maestro, Appium, Tester Army's
e2e and Software Mansion's Argent on the same scenarios and the same device, and awards medals: gold, silver and bronze per metric, Olympic style. Results within
2% of each other tie (both get the higher medal; the next rank is skipped). A tool with no
value for a metric — it does not support it, or failed more than half its runs — gets no
medal for it.

## Metrics

| Group | Metric | Better | What it measures |
|---|---|---|---|
| Start-up | Install size | lower | Bytes on disk needed to drive Android: the binary, plus maestro-runner's Android driver APKs; Maestro's `lib` folder (not the Java runtime); Appium's server and UiAutomator2 driver (not Node.js). |
| Start-up | CLI start | lower | Median wall time of `--version` (10 runs): how heavy the binary is to start. |
| Start-up | Invalid flow reported | lower | Wall time to reject a flow with an unknown command. A tool that exits 0 on it gets no value: CI would call the run green. |
| Start-up | One-command flow | lower | Wall time of a flow with one `pressKey: Home`: agent start, connection, one command, teardown. |
| Execution | App launch, Navigation, Scroll to element, Text input, Assertion burst, Repeated taps | lower | Median wall time of the tool's passing runs of each scenario, start of the command to its exit. |
| Execution | Whole suite | lower | Sum of the scenario medians (only for a tool with a value for each). |
| Resources | Peak host memory | lower | Median of the per-run peak memory of the tool's main process. |
| Resources | Host CPU time | lower | Sum over scenarios of the median CPU time (user + kernel) of the tool and every process it starts. |
| Resources | Peak device agent memory | lower | Median of the per-run highest resident memory of the tool's processes on the device, sampled every 0.5 s. |
| Reliability | Runs passed | higher | Share of all device runs that passed. |
| Reliability | Run-to-run variation | lower | Mean coefficient of variation of wall time over the scenarios with three or more passing runs. |
| Speedrun Bench app | One per flow, Whole app suite, App flows passed | lower (passed: higher) | Wall time of each flow of speedrun's own benchmark app (see below), their sum (only for a tool that passed every flow), and the share of flow runs passed. |
| Tester Army mobile benchmark | One per flow, Whole app suite, App flows passed | lower (passed: higher) | The same, for Tester Army's mobile benchmark app (see below). |
| Features | One per feature in `bench/features.json` where the tools differ | higher | Features every tool supports the same way award no medal. Full support beats partial; no support wins nothing. Each category also shows a score (supported 1, partial ½) and its medal count. |

## Scenarios

`bench/flows/*.yaml`, the same file for every tool, against Android's Settings app (on every
emulator image, so anyone can rerun this):

- `launch` — a cold start, then one assertion.
- `navigate` — three screens one level down and back.
- `scroll` — `scrollUntilVisible` to the bottom of the list, into a screen, then back to the top.
- `input` — type into the search field, erase, type again, hide the keyboard.
- `assertions` — fifteen checks of one screen, with regex, id and relative selectors.
- `taps` — in and out of one screen five times with `repeat`.
- `trivial` — one command: start-up overhead.
- `invalid` — an unknown command: error detection.

Appium has no flow files: `bench/appium/run.mjs` runs the same steps as code, the way an Appium
user writes them (a UiAutomator2 session, UiSelector lookups with an implicit wait). The Appium
server runs in the test's own process and starts and stops with each run, as the other tools'
agents do, so one process is measured. It has no flow to reject, so it skips `invalid`.

### Speedrun Bench app

`--scenarios app` runs every flow of speedrun's own benchmark app
([`bench/app`](https://github.com/hubastard/speedrun/tree/main/bench/app)) as `app/<flow>`: a
React Native app built to be hard to test (screens that never stop moving, popups, lists that
load as they scroll, a custom keypad, a Jetpack Compose screen). Its flows are Maestro flows, so
every flow runner runs them; Appium does not. Set up:

```sh
git -C ../speedrun checkout v0.2.11      # the checkout next to this repository
gh release download bench-app -R hubastard/speedrun -p 'speedrun-bench-<hash>.apk'
adb install -r speedrun-bench-<hash>.apk # <hash>: `bench/app/bench.sh hash` in that checkout
```

`VIRTUOSO_BENCH_APP_FLOWS` points at another flows folder. Maestro takes about 45 minutes for
the suite (over 20 of them on the flow with screens that never stop moving), so run the app
with one iteration: `--scenarios app --iterations 1`.

e2e, Argent and Appium run tests of their own rather than Maestro flows: `bench/e2e/app`,
`bench/argent/app` and `bench/appium/app/scenarios.mjs`, step for step with the app's flows,
with an adb shell script where a tool has no command for a step.

### Tester Army mobile benchmark

`--scenarios tam` runs `bench/tam-flows` as `tam/<flow>`: the scenarios of Tester Army's mobile
benchmark (`apps/mobile-benchmark` in [tester-army/e2e](https://github.com/tester-army/e2e)), an
Expo app of surfaces that are hard to automate (merged and hidden accessibility trees, native
alerts over modals, a keyboard over the submit button, virtualized lists, a WebView, system
permission dialogs). Every tool runs it: Appium from `bench/appium/tam/scenarios.mjs`, e2e and
Argent from their `tam` folders. The Apple Pay scenarios (iOS only) and the Stripe one (card
details in a live payment sheet) are not run. Taps by position are at whole percentages of the
screen, as Maestro and speedrun take no decimals.

For a long run, `../e2e/bench_go.sh` (run from this repository) runs the flows still to run one
at a time, waits for 6 GB of free memory before each, stops if the emulator does not answer, and
restarts the emulator every second flow (`BENCH_RESTART_EVERY`): left running for an hour and a
half it grows past 5 GB and freezes. The report's notes under each app suite give the reasons
behind the failures that remain.

## Fairness

- Before every run the device is reset to the same state: every known tool's test agent
  stopped, whichever tools the run covers (an agent left by an earlier run, such as Argent's,
  holds the UiAutomation connection and blocks the others), adb forwards removed, Settings' data cleared
  (Android otherwise reopens Settings on the last screen visited), animations back to their
  defaults (virtuoso and speedrun turn them off; maestro-runner does not by default), portrait,
  light mode, home screen.
- Each tool runs each scenario once unrecorded first (agent install, file caches), then the
  recorded runs. On Android the tools take turns within each iteration so drift spreads across
  them. On iOS each tool runs its warm-up and recorded runs as one block, after every tool's
  agent is stopped (see *Agents between runs* below).
- A tool is never measured on its own against others measured together: a rerun covers every
  tool, so all of them see the same device state (Android's Apps screen lists recently opened
  apps only once some app has been opened) and the same timing.
- Every tool drives the same emulator through the same adb server.
- Results accumulate across benchmark runs (`--merge-into`). speedrun and maestro-runner keep
  (and every other competitor) keep every batch recorded at their current version and are
  scored on their best: per scenario,
  the batch with the fastest median, and the lowest memory and CPU medians of any batch. virtuoso
  is scored on its latest batch only, so a virtuoso regression shows straight away. A new version
  of a competitor discards its earlier batches.

## On GitHub Actions, per platform

The published results come from three workflows, each run by hand (`workflow_dispatch`), so
anyone can rerun them on the same machines:

| Workflow | Field | Device | Tools |
|---|---|---|---|
| `android-bench.yml` | Android (Linux runner) | Android 16 emulator on KVM, `ubuntu-latest` | all seven, natively |
| `android-bench-windows.yml` | Android (Windows runner) | Android 16 emulator on WHPX, `windows-latest` | all but speedrun; virtuoso's Linux build in WSL 1 and its Windows build natively |
| `ios-bench.yml` | iOS (macOS runner) | iPhone simulator (newest iOS on the image), `macos-15` | all seven, each in a block of its own |

Each workflow runs one job per group of scenarios (the Settings scenarios together, then each
app flow alone), every job on its own freshly booted device with every tool taking turns on
it, so a comparison never spans two machines. A final job merges the jobs' results
(`virtuoso-bench merge`) and renders the report. `virtuoso-bench readme --field
"<name>=<results.json>"...` then writes the README's medal tables and a chart per field into
`docs/bench/`.

**Each field scores separately.** A performance metric is a medal event of its own in each
field (Android on a Linux runner, Android on a Windows runner, iOS on a Mac), so a tool that
wins on one platform and not another shows it. The 56 feature events count once. The charts
show each tool's time against virtuoso's: the geometric mean, over the scenarios both passed,
of the tool's time over virtuoso's.

What a field leaves out, and why:

- **speedrun on Windows runners.** speedrun is POSIX-only, so on Windows it runs in WSL. WSL 1's
  kernel cannot run it ("Flow directories do not contain any Flow files", even for a flow in
  WSL's own filesystem); WSL 2 can, but its mirrored networking is not available on these
  runners (`ConfigureNetworking 0x803b0015`), and without it WSL 2 cannot reach the ports the
  Windows adb server forwards to each tool's on-device agent.
- **On iOS, Argent's view of Settings.** Settings is an Apple system app, and Argent's runner
  reads no view tree from one, so its tests of it (`bench/argent/settings-ios`) take Argent's
  documented tree-free form: accessibility checks (`await-ui-element`), and taps and swipes at
  the rows' points on the runner's simulator. They start Settings with `restart-app`, as a
  `launch:` step on a system app waits out a devtools connection it never makes (~16 s). That
  form has no relative check, so Argent's assertions scenario leaves out "Accessibility below
  General". e2e's tests of iOS's Settings (`bench/e2e/settings-ios`) find rows by text, as on
  Android.
- **The iOS Settings scenarios** are their own flows (`bench/flows-ios`), against iOS's
  Settings app; iOS has no Back key, so they go back with the swipe in from the screen's left
  edge, which every tool can make. The simulator has no VoiceOver, so Accessibility is checked
  by what it does show.
- **Agents between runs.** The Android reset stops every tool's on-device agent before each run.
  On iOS a tool's agent is an XCTest runner, kept up between runs (speedrun's and virtuoso's
  are; starting one through xcodebuild takes most of half a minute), and an idle runner slows
  whichever tool runs next (virtuoso's navigation took 27.6 s with speedrun's runner up beside
  it, 21.1 s without). So on iOS each tool runs as a block: every `xcodebuild
  test-without-building` host is stopped first (a host relaunches a stopped runner), then every
  runner on the simulator, then the tool's warm-up starts its own agent and its recorded runs
  follow. The iOS reset between runs clears the apps' state and leaves the running tool's agent
  up, as its user would see on their second test.

The CI emulators draw in software (SwiftShader), so their times are slower than a desktop
with a GPU, and runners are shared machines: absolute times vary from run to run, the
rankings much less.

## Where each tool runs on the reference machine (Windows 11)

speedrun is macOS/Linux only, so it runs in WSL 2. virtuoso's Linux build runs there too, so
the two compare like for like (same OS, same measurement: GNU time inside Linux, which leaves
out wsl.exe's start-up). maestro-runner runs natively on Windows: its Linux build tests a
local port before `adb forward` binds it on the Windows side, and WSL's proxy holds every
port so tested. Native runs are measured through the Windows process handle (peak working set)
and a job object (CPU time of the whole process tree). The report adds virtuoso's native Windows
build against maestro-runner as a supplement, outside the medal tables.

## Running it

Prerequisites on the reference machine:
- an emulator (`emulator-5554`);
- a WSL distribution (`VirtuosoUbuntu`) with GNU time, a Linux adb of the same version as
  Windows' (talking to the Windows server), and speedrun (v0.2.11) and a Linux virtuoso build
  under `/opt/bench`;
- maestro-runner built into `bench/tools/maestro-runner.exe`, with its checkout next to this
  repository (for its `drivers/`);
- Maestro 2.11.0 unpacked into `bench/tools/maestro-2.11.0` (the `maestro.zip` of its
  `cli-2.11.0` release; `MAESTRO_HOME` points elsewhere) and a Java 17 runtime on `PATH`. The
  harness starts its JVM directly, as `maestro.bat` would, so the process measured is Maestro's;
- Appium: `npm install` in `bench/appium` (Appium 3.9.0), and its UiAutomator2 driver
  (`appium driver install uiautomator2`, into `APPIUM_HOME`, by default `~/.appium`);
- e2e and Argent: `npm install` in `bench/e2e` (e2e 0.22.0, @e2e-dev/mobile 0.12.5) and in
  `bench/argent` (@swmansion/argent 0.31.1).

Build the two virtuoso binaries first, with `sh bench/build.sh`:
- It builds the native Windows release, which builds the agent.
- It copies that agent jar to `bench/tools/virtuoso-agent.jar`.
- It builds the Linux release in WSL with the same jar into `/opt/bench/virtuoso`. That build is
  statically linked against musl, non-PIE, and stripped of unwind tables and notes. It needs
  `musl-tools` and `lld` in the distribution (see `docs/install-size-backlog.md`).
- It checks the Linux binary against its size budget (`crates/virtuoso/tests/size.rs`).
- Set `VIRTUOSO_BENCH_WSL` when the distribution has another name.

```sh
sh bench/build.sh
target/release/virtuoso-bench run --iterations 5 --out bench/results/results.json
target/release/virtuoso-bench report bench/results/results.json --out report/index.html
```

`--tools virtuoso,speedrun` and `--scenarios navigate,scroll` narrow a run. `--version` is
measured on its own, so it is not a scenario.

`report` also rewrites the results section of `README.md`, between the
`<!-- benchmark:start -->` and `<!-- benchmark:end -->` markers. That section holds the
medal tables and every performance metric, with the winner in bold. `--readme <path>` updates
a different file, and `--no-readme` leaves the README alone.

Guards against measuring the wrong build:
- **Newest build:** `virtuoso-windows` runs the most recently built `target/*/release/virtuoso.exe`,
  or `VIRTUOSO_WINDOWS_BIN` when that is set.
- **Same agent:** the run is refused when the virtuoso builds name different agents in
  `--version`. Builds with different agents delete each other's agent jar on the device and
  push their own on every run.
- **No child processes:** each native run records how many processes it started. A virtuoso
  run that started more than itself is flagged in the progress output.

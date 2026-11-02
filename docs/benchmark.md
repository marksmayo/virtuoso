# The benchmark

`virtuoso-bench` compares virtuoso, maestro-runner and speedrun on the same flows and the same
device, and awards medals: gold, silver and bronze per metric, Olympic style. Results within
2% of each other tie (both get the higher medal; the next rank is skipped). A tool with no
value for a metric — it does not support it, or failed more than half its runs — gets no
medal for it.

## Metrics

| Group | Metric | Better | What it measures |
|---|---|---|---|
| Start-up | Install size | lower | Bytes on disk needed to drive Android: the binary, plus maestro-runner's Android driver APKs. |
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

## Fairness

- Before every run the device is reset to the same state: test agents stopped (virtuoso's,
  speedrun's, maestro-runner's instrumentation), adb forwards removed, Settings' data cleared
  (Android otherwise reopens Settings on the last screen visited), animations back to their
  defaults (virtuoso and speedrun turn them off; maestro-runner does not by default), portrait,
  light mode, home screen.
- Each tool runs each scenario once unrecorded first (agent install, file caches), then the
  recorded runs. Tools take turns within each iteration so drift spreads across them.
- Every tool drives the same emulator through the same adb server.
- Results accumulate across benchmark runs (`--merge-into`). speedrun and maestro-runner keep
  every batch recorded at their current version and are scored on their best: per scenario,
  the batch with the fastest median, and the lowest memory and CPU medians of any batch. virtuoso
  is scored on its latest batch only, so a virtuoso regression shows straight away. A new version
  of a competitor discards its earlier batches.

## Where each tool runs on the reference machine (Windows 11)

speedrun is macOS/Linux only, so it runs in WSL 2. virtuoso's Linux build runs there too, so
the two compare like for like (same OS, same measurement: GNU time inside Linux, which leaves
out wsl.exe's start-up). maestro-runner runs natively on Windows: its Linux build tests a
local port before `adb forward` binds it on the Windows side, and WSL's proxy holds every
port so tested. Native runs are measured through the Windows process handle (peak working set)
and a job object (CPU time of the whole process tree). The report adds virtuoso's native Windows
build against maestro-runner as a supplement, outside the medal tables.

## Running it

Prerequisites on the reference machine: an emulator (`emulator-5554`), a WSL distribution
(`VirtuosoUbuntu`) with GNU time, a Linux adb of the same version as Windows' (talking to the
Windows server), speedrun and a Linux virtuoso build under `/opt/bench`, and maestro-runner built
into `bench/tools/maestro-runner.exe` with its checkout next to this repository (for its
`drivers/`).

Build the two virtuoso binaries first, with `sh bench/build.sh`:
- It builds the native Windows release, which builds the agent.
- It copies that agent jar to `bench/tools/virtuoso-agent.jar`.
- It builds the Linux release in WSL with the same jar into `/opt/bench/virtuoso`. That build is
  statically linked and non-PIE, so it has no dynamic loader or start-up relocations.

```sh
sh bench/build.sh
target/release/virtuoso-bench run --iterations 5 --out bench/results/results.json
target/release/virtuoso-bench report bench/results/results.json --out report/index.html
```

`--tools virtuoso,speedrun` and `--scenarios navigate,scroll` narrow a run. `--version` is
measured on its own, so it is not a scenario.

Guards against measuring the wrong build:
- **Newest build:** `virtuoso-windows` runs the most recently built `target/*/release/virtuoso.exe`,
  or `VIRTUOSO_WINDOWS_BIN` when that is set.
- **Same agent:** the run is refused when the virtuoso builds name different agents in
  `--version`. Builds with different agents delete each other's agent jar on the device and
  push their own on every run.
- **No child processes:** each native run records how many processes it started. A virtuoso
  run that started more than itself is flagged in the progress output.

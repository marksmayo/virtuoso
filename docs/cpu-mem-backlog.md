# CPU and memory backlog: 100 zero- to low-risk improvements

Sorted by expected gain. Scope: host CPU, host peak memory, and the on-device agent's memory
and CPU, as the benchmark measures them.

## Where virtuoso stands

Benchmark medians (all batches in `bench/results/results.json`):

| | virtuoso (Linux, WSL) | virtuoso (Windows) | speedrun | maestro-runner |
|---|---|---|---|---|
| Host CPU per flow | 8–30 ms | 141–297 ms | 400–1200 ms | ~2000 ms |
| Host peak RSS, device flow | 6.4–7.1 MB | 7.2–8.1 MB | **5.9 MB** | 22–25 MB |
| Host peak RSS, `--version` | 4.0 MB | 4.8 MB | **2.8 MB** | 12.2 MB |
| Agent RSS on device | **133–141 MB** | | 147–154 MB | 140–173 MB |
| Install size | 4.9 MB | 4.2 MB | **3.6 MB** | 49 MB |

What the survey found (measured on this machine, 2026-11-02):
- **The Windows figure is mostly a stale binary.** The bench runs `target/release/virtuoso.exe`
  (built 2026-11-01 23:55), which predates `adb_wire` and spawns `adb.exe` for every adb
  operation (50–60 ms of job CPU each). The current code against a fake device uses 11–47 ms
  of job CPU per flow and spawns no child process.
- **Two agent jars.** `target/release` embeds a different agent jar from `target/dev3` and
  `bench/tools/virtuoso-agent.jar` (which the Linux build uses). The agent start deletes every
  other build's jar, so the Windows and Linux builds can delete each other's jar and push on
  every iteration (+26 to +45 ms job CPU and extra wall time per run, measured on a fake).
- **Start-up memory is the dynamic loader and relocations.** A static, non-PIE Linux build
  with fat LTO measured 2776 KB for `--version` (speedrun 2780) against 3992–4108 KB today.
  `opt-level = "s"` shrinks the binary 22% but does not lower RSS.
- **During a run** the extra memory is QuickJS (147 KB of heap plus code, built even when no
  flow uses JS), a `serde_json::Value` tree per screen read (~221 KB for 250 nodes, against
  ~31 KB typed), and `Pattern::new` (~340 KB transient per new selector, 22 KB kept, cache of
  512).
- **On the device** most of the agent's RSS is shared framework memory the agent cannot change.
  What it can change is its private heap and the CPU it takes from the app: a full screen dump
  every 10 ms while waiting, all accessibility event types subscribed, a 32 KB buffer per
  reply, and the screenshot path (10 MB bitmap, ~8× the PNG in temporary copies).

How to measure:
- Host: `/usr/bin/time -f %M` in WSL; peak working set and job CPU (with `TotalProcesses`) on
  Windows.
- Device: `cat /proc/$(pidof virtuoso-agent)/status` (`RssAnon`, `Threads`) and `smaps_rollup`
  for memory; `/proc/<pid>/stat` utime+stime over a fixed flow for CPU, for the agent and for
  the app under test.

**Expected gain** is per flow unless stated. Marked *(m)* when measured, *(g)* when a guess.
W = native Windows build, L = Linux build. Items that only help a command the bench does not
run (screenshots, `--video`, `http`) are ranked by their gain when it does run, below items
that help every flow.

## Status (2026-11-02)

Items 1–19 are done, each test first. All virtuoso suites pass, including every device test
(run twice), and clippy is clean.

| # | How it was proven | Result |
|---|---|---|
| 1 | Bench test: picks the newest `target/*/release` build | the bench no longer runs a stale binary |
| 2 | `/usr/bin/time` on the Linux build (red: 3972–4132 KB, dynamically linked) | `--version` 2936 KB, invalid flow 3896 KB; binary 5.9 MB, up from 4.9 MB |
| 3 | Bench test: a run is refused when the virtuoso builds name different agents; `bench/build.sh` builds both with one jar | both builds report the same agent |
| 4 | Device test: another build's agent stays, a stale one goes | |
| 5–7 | Device tests: a 1 s wait for a missing element sent 66 requests (red), now ≤ 20; hiding the keyboard sent 6, now ≤ 4 | the device flow suite went from 101–138 s to 41 s |
| 8 | Fake-server test of the sync protocol; device tests: a push from memory lands, and a missing agent is pushed and starts | |
| 9 | CLI test: `--version` names the agent (`virtuoso 0.1.0 (agent …)`) | |
| 10 | Measure test: native runs count the processes they started | |
| 11 | JS test: no engine until a flow uses JavaScript | |
| 12 | Golden test: a typed parse gives the same elements as the `Value` path | |
| 13 | Select test: plain text compiles no regex | |
| 14 | Device test: the subscribed event types are the cache-critical ones | |
| 15 | CLI test: `virtuoso.exe` does not import winmm.dll (red: it did) | |
| 16–19 | Device tests: screenshot PNG, text outside the BMP; `agent_memory` measurement (ignored test) | after a screenshot, agent RSS 148–149 MB (was 162), RssAnon 47–48 MB (was 60) |

Check run of both virtuoso builds afterwards (3 iterations, medians, against the earlier
benchmark's virtuoso medians):

| | Before | After |
|---|---|---|
| Host peak RSS, Linux build, device flows | 5.9–6.8 MB | 3.8–4.9 MB (speedrun 5.9 MB) |
| Host peak RSS, Windows build, device flows | 7.2–8.0 MB | 6.6–7.2 MB |
| Host CPU, Windows build | 141–297 ms | 0–31 ms, one process |
| Host CPU, Linux build | 8–30 ms | 2–7 ms |
| Agent RSS | 133–140 MB | 133–138 MB |

Text input took longer in the check run: 5.6–9.8 s against 4.2 s before. speedrun was slower
by a similar share on the same emulator at the same time (6.3–7.8 s against its best of
5.4 s). The slow part, the first `type` waiting for input focus, is just as slow with the
event filter (14) removed. Nothing else changed in typing. So the emulator had slowed and
virtuoso had not regressed; a full benchmark rerun on a freshly booted emulator will confirm.

## The list

| # | Improvement | Where | Expected gain | Risk | Red/green |
|---|---|---|---|---|---|
| 1 | Benchmark the current build: point `virtuoso-windows` at the freshly built binary (or rebuild `target/release`) before every run | `virtuoso-bench/src/main.rs:33`, `docs/benchmark.md` | W CPU 141–297 → 11–47 ms *(m)* | Zero | Bench test: the tool path is the newest build; `strings` finds `host:transport:` |
| 2 | Static glibc + non-PIE + fat LTO for the Linux release (`--target x86_64-unknown-linux-gnu`, `+crt-static`, `relocation-model=static`) | `.cargo/config.toml`, `Cargo.toml:12` | L RSS −1.0 to −1.3 MB at start-up and in every flow *(m)*; binary +1.0 MB | Low (no ASLR for the exe; std uses no NSS here) | `/usr/bin/time` `--version` ≤ 2900 KB; `file` says statically linked |
| 3 | Build Linux and Windows with the same agent jar (`VIRTUOSO_AGENT_JAR`) for the bench | `docs/benchmark.md`, bench build steps | W 26–45 ms CPU per run *(m)*, plus a push's wall time; L similar *(g)* | Zero | Both `--version` builds report the same jar hash (see 9) |
| 4 | Stop deleting other builds' agent jars on every start (keep them, or delete only old ones) | `android.rs:244` | Same as 3 whenever two builds share a device | Low (15 KB per jar on device) | Device test: start A, B, A; the third start does not push |
| 5 | Agent: skip unchanged dumps (`screen` with `ifUnchanged` → `{"unchanged":true}` when no event since the last dump and no Compose) | `Screen.java:53`, `Settle.java`, `android_device.rs:222` | Device CPU during element waits −50 to −90% *(g)*; host parse skipped per repeat | Low (only views changing without events; Compose handled) | Device test: two reads with no change, second is `unchanged`; text change gives a full dump |
| 6 | Long-poll lookups: `screen` takes `waitMs`, the agent blocks until an event or timeout; the host drops its 10 ms sleep | `Server.java:117`, `runner.rs:737-765,1005` | Up to 10× fewer dumps while waiting *(g)*; host W 2–10 ms *(g)* | Low (timeout covers Compose) | Device test: element appearing after 1 s found within ~20 ms; dump count per wait |
| 7 | Event-gated `windows` waits: `wait_for_app` and `hide_keyboard` wait on the agent for the condition instead of polling every 10 ms | `android_device.rs:238-254,424-437`, `Screen.java:67` | ~90% of those calls *(g)*; host W 3–6 ms on input *(g)* | Low (old op kept) | Device test: keyboard hides; round trips in a `VIRTUOSO_DEBUG` trace |
| 8 | Push the agent jar over adb wire `sync:` from memory (no temp file, no `adb.exe`) | `adb_wire.rs`, `android.rs:250-254` | W ~50 ms CPU per push *(m: spawn cost)* | Low (falls back to `adb push`) | Fake-server test; job `TotalProcesses` = 1 on a fresh device |
| 9 | Put the agent jar hash and git hash in `--version`; the bench checks the binary matches the tree | `main.rs:22`, `build.rs` | Enabler: catches 1 and 3 | Zero | CLI test: `--version` contains the jar hash |
| 10 | Record the job's `TotalProcesses` per run and flag virtuoso runs that spawned children | `virtuoso-bench/src/measure.rs:175` | Enabler: catches spawn regressions | Zero | Unit test on the record field |
| 11 | Create QuickJS on first use (`OnceCell`); queue `set_var` until then | `js.rs:28-50`, `runner.rs:102,142-156` | RSS −350 to −650 KB per device flow *(m 147 KB heap + g code pages)*; CPU ~1 ms *(m)* | Low | Test: a flow with no `${}` never builds the engine |
| 12 | Parse screen replies into typed `Deserialize` structs instead of a `Value` tree | `agent.rs:26-36`, `android_device.rs:222`, `screen.rs:107-140` | RSS −190 to −260 KB per read of 250 nodes *(m)*; CPU 169 → 51 µs per 16 KB dump *(m)* | Low (defaults must match) | Golden test: same `ScreenTree` from the same JSON |
| 13 | Literal fast path in `Pattern`: no regex compile when the pattern has no metacharacters | `select.rs:24-27` | RSS −340 KB transient per new plain selector *(m)*; CPU ~0.2 ms each *(m)* | Low (rare Unicode case folding) | Select tests incl. a Unicode case; probe shows ~0 bytes |
| 14 | Agent: subscribe only to the event types used (`info.eventTypes`) | `Main.java:59-63` | Fewer events built by the app and delivered to the agent, 5–20% *(g)* | Low (verify focus/checked still update) | Device test: focused and checked flags appear after taps |
| 15 | Windows: load winmm only for device runs (`timeBeginPeriod` through `LoadLibraryW`) and `timeEndPeriod` at the end | `main.rs:181-185`, `Cargo.toml:32` | W RSS −250 KB for `--version`, lint and invalid *(m)*; machine-wide timer rate off outside runs | Low | Peak WS of `--version` < 4900 KB |
| 16 | Agent: recycle the screenshot bitmap after `compress` | `Screen.java:163-167` | ~10 MB lower agent peak after a screenshot *(g)* | Zero | `RssAnon` after `screenshot`, before and after |
| 17 | Agent: send screenshots as a binary frame, not base64 inside JSON | `Screen.java:167`, `Server.java:94`, `android_device.rs:439` | Agent 5–10 MB transient per screenshot *(g)*; host 0.3–1.5 MB | Low (host and agent ship together) | Device test: decoded PNG has display size |
| 18 | Agent: reuse one `Json.Out` per connection (`setLength(0)`; shrink after a huge dump) | `Json.java:107`, `Server.java:79` | ~3 MB/s less garbage while polling; heap high-water −0.5 to −2 MB *(g)* | Zero | Byte-identical replies; GC count in logcat |
| 19 | Agent: encode replies into a kept byte buffer, not `toString().getBytes()` | `Server.java:93-96` | Two fewer copies per reply; −0.3 to −1 MB peak on big dumps *(g)* | Zero (handle surrogates) | Emoji round-trip test |
| 20 | Agent: remember each window id's package; `getRoot()` only for unseen ids | `Screen.java:72-89` | Most of the `windows` op's cost *(g)* | Low | Device test: package right after app B opens over A |
| 21 | Agent: on Compose screens clear only that window's cached subtree (API 34+) | `Screen.java:44-51` | 20–40% fewer IPCs into SystemUI per dump *(g)* | Low | Compose text change still seen |
| 22 | `wait_for_app`: full `screen()` for an error dialog at most once per wait | `android_device.rs:249-251` | Up to hundreds of dumps per launch when a system window is present *(g; trace first)* | Low | FakeDevice with a system window: read count |
| 23 | `Cmd::Alert`: one `find_within`-style wait instead of a full read every 100 ms for 7 s | `runner.rs:500-515` | Up to 70 reads; W ~25–35 ms CPU when no alert *(g)* | Low | FakeDevice counts `screen()` calls |
| 24 | Agent: rate-limit settle's `idle()` probes to one per frame (16 ms) | `Settle.java:87-89,119` | Up to 3× fewer probes into the app's main thread *(g)* | Low (settle ≤ 11 ms later) | Settle `probes` drops; bench timings within noise |
| 25 | Agent: settle waits on a monitor instead of a 5 ms poll loop | `Settle.java:104` | 200 fewer wake-ups a second during settle *(g)* | Low | Device settle tests unchanged |
| 26 | Agent: hash signatures to `long` during restless settles | `Screen.java:151-160`, `Settle.java:134-156` | 2–5× less CPU and garbage during animations *(g)* | Low | `Restless` unit test; spinner fixture still `stable` |
| 27 | Agent: wait for focus events instead of `findFocus` every 5 ms | `Input.java:187-194` | Up to 400 IPCs per worst-case wait *(g)* | Low | Device test: type into a field opened by a tap |
| 28 | Agent: append escape-free strings whole in the JSON writer | `Json.java:129-146` | 10–30% of dump build time *(g)* | Zero | Escaper unit test; `us` on a 500-node dump |
| 29 | Agent: no-prefetch `getRoot(0)` in the `windows` path (API 33+) | `Screen.java:77` | App main-thread time per cache miss *(g)* | Low | `windows` output unchanged |
| 30 | Agent: depth-first prefetch in `walk` (measure against the hybrid default) | `Screen.java:123` | 0–30% fewer IPCs on a cold dump *(g)* | Low | Dump identical; `us` of first dump after navigation |
| 31 | Agent: `trimHeap()` after a reply over ~1 MB or a screenshot | `Server.java` | 5–20 MB after screenshots or big dumps *(g)* | Low | `RssAnon` 2 s after a screenshot |
| 32 | Agent: `setTargetHeapUtilization(0.9f)` at start | `Main.java` | Agent heap −1 to −4 MB *(g)* | Low (slightly more GC) | Steady-state `RssAnon` |
| 33 | Agent: `updateProcessState` to an imperceptible state so ART trims and compacts | `Main.java` | Agent −2 to −8 MB *(g)* | Low (check reply p99) | `RssAnon` and bench p99 |
| 34 | Agent: keep the GPU stack out (screencap fallback) if the first `takeScreenshot` maps it in — measure first | `Screen.java:163` | 10–30 MB that stays after the first screenshot *(g)* | Low (fallback kept) | RSS before/after one screenshot and 60 s later |
| 35 | Agent: raw deflated pixels at `BEST_SPEED`, PNG encoded on the host | `Screen.java:166` | 100–300 ms device CPU per screenshot *(g)* | Low | Same pixels on the host |
| 36 | Shrink the host pattern cache from 512 to ~64 entries (LRU) | `select.rs:36` | Up to ~11 MB kept in long suites; ~0 in the bench *(m: 22 KB per entry)* | Zero | Cache length stays ≤ cap |
| 37 | Non-PIE only, if a static libc is unwanted | rustflags | L RSS −350 KB, binary −310 KB *(m)* | Low | `size -A` relro < 50 KB |
| 38 | RELR packed relocations (alternative to 37) | rustflags | L RSS −290 KB, binary −295 KB *(m)* | Low (glibc ≥ 2.36) | `readelf -S` |
| 39 | `lto = "fat"` | `Cargo.toml:12` | L RSS −250 KB, W −50 to −80 KB, binary −95 to −165 KB *(m)* | Zero | Size and `/usr/bin/time` |
| 40 | Streaming `assertScreenshot` comparison (row by row) | `runner.rs:367-369` | Host −20 MB for that command *(g)* | Low | Same similarity % |
| 41 | Streaming crop for `cropOn` | `runner.rs:619-626`, `image_util.rs:12-25` | Host −8 to −10 MB for that command *(g)* | Low | Pixel-equal crop |
| 42 | Wire fallback first: when the wire can't connect, `adb start-server` once and retry, not a spawn per operation | `android.rs:96-101,145-150,309-325` | W 120+ ms when no server runs *(g)* | Low | Server stopped: 2 processes in the job |
| 43 | `tcp:0` forward fallback over the wire | `android.rs:267` | W ~60 ms when the fixed port is taken *(g)* | Low | Fake server FAILs the fixed port; no child |
| 44 | `--app-file` install over the wire (`exec:cmd package install -S`) | `main.rs:744` | W ~60 ms per APK *(g)* | Low | Fake-server `exec:` test |
| 45 | `addMedia` push over wire sync | `android_device.rs:515` | W ~60 ms per file *(g)* | Low | As 8 |
| 46 | `--video`: start through a wire shell, pull through wire sync | `android_device.rs:562-569,579` | W ~120 ms per flow with `--video` *(g)* | Low | Job procs = 1 with `--video` |
| 47 | Emulator boot polling through the wire shell | `android.rs:83` | W ~60 ms per second of boot *(g)* | Low | `--auto-start-emulator`: only the emulator spawns |
| 48 | Agent: drop `String.format` from the JSON escaper | `Json.java:140` | 0.1–0.5 MB of Formatter classes once triggered *(g)* | Zero | `"\u0001"` escapes the same |
| 49 | Agent: smaller exec buffers (16 KB chunk, size from the first read) | `Exec.java:20-22` | ~100 KB churn per exec *(g)* | Zero | `dumpsys package` output unchanged |
| 50 | Agent: run argv directly when no shell features are needed | `Exec.java`, `exec` callers | 2–5 ms device CPU per avoided `sh` *(g)* | Zero/Low | Same exit code and output |
| 51 | Agent: cap binder threads (`BinderInternal.setMaxThreads(4)`) | `Main.java` | 0.1–1 MB *(g)* | Low | `Threads:` after an event flood |
| 52 | Agent: VM heap options on the `app_process` line (check they take effect) | `android.rs:246` | 1–5 MB if effective *(g)* | Low | Log effective values; `RssAnon` |
| 53 | Agent: 8 KB socket output buffer instead of 64 KB | `Server.java:65` | 56 KB | Zero | Tests pass |
| 54 | Agent: smaller dump output (drop per-node `"w"`, delta `"d"`) | `Screen.java:110`, `screen.rs` | 5–10% fewer reply bytes to build, send and parse *(g)* | Low (both sides change) | Host parse tests; bytes per dump |
| 55 | Agent: cache the `clearCache` `Method` | `Screen.java:47` | Reflection per dump | Zero | Unchanged |
| 56 | Agent: cache the display lookup | `Screen.java:34-42` | Reflection per `info` poll | Zero | Rotation test |
| 57 | Agent: cache Clipboard reflection | `Clipboard.java:34` | `getMethods()` per call | Zero | Clipboard test |
| 58 | Agent: store the dex uncompressed (or push a bare `.dex`) | `build.rs:66-70`, `android.rs:246` | Tens of KB private dirty | Zero | Agent starts; `RssAnon` |
| 59 | Agent: no per-character allocations in `type` | `Input.java:156-159` | Tiny | Zero | Non-BMP typing test |
| 60 | Agent: JIT off vs on — measure | `android.rs:246` | ±2–4 MB, may cost CPU *(g)* | Low | `RssAnon` and dump `us` |
| 61 | Agent: lower nice value — measure | `android.rs:246` | Shifts CPU to the app *(g)*; may slow steps | Low | App frame times vs step latency |
| 62 | Delay-load `ws2_32.dll` on Windows | rustflags `/DELAYLOAD` | W −100 to −300 KB for `--version`/lint *(g)* | Low | Peak WS `--version` |
| 63 | Trim clap features (no color, no suggestions) | `Cargo.toml:24` | RSS −40 to −80 KB, binary −36 to −55 KB *(m)* | Low (plain help) | `--help` golden test |
| 64 | `Context::base` plus only the intrinsics the prelude needs | `js.rs:30` | Up to −40 KB per engine *(m)* | Low | JS tests |
| 65 | QuickJS GC threshold 64 KB and a memory limit | `js.rs:29` | 0–200 KB in script-heavy flows *(g)* | Zero/Low | Allocating loop stays bounded |
| 66 | One QuickJS engine per device, reset between flows (or drop before the next) | `main.rs:833` | 0–150 KB, ~1 ms per retry *(g)* | Low | No `output` leaks between attempts |
| 67 | Cheaper regex build limits | `select.rs:25` | 50–200 KB transient *(g)* | Low | Select tests |
| 68 | Reuse one frame buffer in `AgentClient` | `agent.rs:33,533-553` | 0–150 KB heap high-water; 2 allocations per call *(g)* | Zero | Protocol tests |
| 69 | `BufReader` 64 KB → 8 KB | `agent.rs:49` | ~56 KB | Zero | `latency` p50 unchanged |
| 70 | Cap glibc arenas and fix the mmap threshold (`mallopt`) | `main.rs` (Linux gnu) | 50–300 KB on device runs *(g)* | Low | `/usr/bin/time` on a device flow |
| 71 | Borrow or take the screenshot base64 instead of copying | `android_device.rs:439-443` | 0.3–1.5 MB on screenshot paths *(g)* | Low | Same bytes |
| 72 | Decode base64 and drop the frame first | `android_device.rs:439-443` | Up to 1× PNG size *(g)* | Zero | As 71 |
| 73 | `find` returns the `Rect` only; drop the tree before tap/settle | `runner.rs:731` | ~70–100 KB (one tree) | Zero | Runner tests |
| 74 | Typed replies for `windows`, `settle`, `exec`; take the windows array instead of `.cloned()` | `android_device.rs:257-260,346-353` | <0.5 ms; 1–5 KB per poll | Zero | Existing tests |
| 75 | Elements hold a window index (or `Rc<str>`) instead of a package `String` each | `screen.rs:135` | 10–20 KB per read; 156 allocations *(m)* | Low | Screen tests |
| 76 | `Box<str>` or one string arena per `ScreenTree` | `screen.rs` | 5–30 KB per read *(g)* | Low | Screen tests |
| 77 | ASCII `eq_ignore_ascii_case` / allocation-free compare in `Pattern::matches` | `select.rs:48,244` | 10–50 µs per poll; ~300 allocations *(g)* | Zero | Select tests |
| 78 | Compute matchable flags and lowercase labels once per tree | `screen.rs:147`, `select.rs:253` | <0.1 ms | Zero | Select tests |
| 79 | Skip `devices()` when `--device` or `ANDROID_SERIAL` is set | `main.rs:459-480` | 0.1–0.3 ms, one Winsock start-up earlier *(g)* | Low (error wording) | Bad serial still exits 2 |
| 80 | Do the forward on the calling thread; drop the forward thread | `android.rs:234-237,264` | One thread, 0.05–0.1 ms W *(g)* | Zero | Agent start test |
| 81 | Wire case: read the ready line with a read timeout, no reader thread | `android.rs:327-350` | One thread and arena, 10–40 KB *(g)* | Low (keep draining) | Agent start/restart tests |
| 82 | 64 KB stacks for the remaining helper threads | `android.rs:236,328` | 0–16 KB | Zero | Agent start test |
| 83 | Retry path keeps the serialized bytes, not `req.clone()` | `android_device.rs:151-152` | <0.1 ms, <1 KB per call | Zero | Agent-lost test |
| 84 | Skip `flush()` work in `Drop` when nothing is pending | `android_device.rs:62` | ~0.3 ms teardown *(g)* | Zero | Cycle trace |
| 85 | `runShell`: blocking wait with timeout instead of `try_wait` every 10 ms | `runner.rs:1041-1050` | ~1 ms per second of command *(g)* | Zero | runShell tests |
| 86 | `tap_when_uncovered` and `set_orientation` use the agent wait (6/7) | `runner.rs:816-840`, `android_device.rs:454-464` | Small; 0 in the bench | Low | FakeDevice counts |
| 87 | Compute the jar hash and remote name in `build.rs` | `android.rs:231` | ~15 µs per start | Zero | Name unit test |
| 88 | Locked, buffered stdout for the live view | `main.rs:669-671` | ~0.05 ms per step *(g)* | Zero | CLI output test |
| 89 | Don't `format!` a step line that won't print | `main.rs` `Ui::step` | Churn only | Zero | Output golden test |
| 90 | Move flows into the queue instead of cloning | `main.rs:795-802` | A few KB per flow | Zero | Sharding tests |
| 91 | Cache parsed sub-flows inside `repeat` | `runner.rs:552-556` | ~40 µs per iteration; 0 in the bench | Zero | Parse-count test |
| 92 | `--continuous`: OS change notifications or a longer interval | `main.rs:690-697` | Idle CPU only | Zero | Change-detection unit test |
| 93 | Per-package `opt-level = "s"` for cold dependencies (rquickjs-sys, clap_builder, unsafe-libyaml, regex-syntax, png) | `Cargo.toml` profile | Binary −300 to −600 KB *(g)*, ~0 RSS | Zero (check timings) | Size; bench timings |
| 94 | One `miniz_oxide` (png 0.18 or matching flate2) | `Cargo.toml:22` | Binary −50 to −120 KB *(g)* | Low | `cargo tree -d`; image tests |
| 95 | Drop the direct `regex` dependency (small glob matcher) | `Cargo.toml:21`, `workspace.rs:58-79` | Binary −20 to −80 KB *(g)* | Low | Glob tests |
| 96 | `-C force-unwind-tables=no` (panic=abort already) | rustflags | Binary −250 KB, ~0 RSS | Low (worse backtraces) | `size -A` |
| 97 | Lazy `MAESTRO_*` env collection | `main.rs` | <1 KB | Zero | Test |
| 98 | `--format` reports built once from the in-memory result | `main.rs:909` | Only with `--format` | Zero | Report tests |
| 99 | Build id off, shorter panic text | linker flags | Binary −5 to −20 KB | Zero | Size |
| 100 | `http.*`: one curl per script, or a small Rust client behind a feature | `js.rs:166-209` | 20–40 ms W CPU per request *(g)*; 0 in the bench | Low/Medium (TLS dependency) | JS http test against a local server |

## Checked and left out

- **`timeBeginPeriod` for CPU:** no job-CPU change measured (it is still worth scoping, see
  15, for memory and the machine-wide timer).
- **Literal pattern fast path for CPU:** under 1 M cycles in place, because patterns are
  already cached per thread. It stays in the list (13) for memory.
- **`opt-level = "s"` for the whole binary:** −22% size, no RSS gain.
- **Agent node `recycle()`, `KeyCharacterMap`, `sources` map:** already fine (`recycle` is a
  no-op on API 33+, the map is cached, `sources` is cleared on every mark).
- **Not low risk:** dropping `FLAG_INCLUDE_NOT_IMPORTANT_VIEWS`, a non-zero
  `notificationTimeout`, precompiling the agent with `dex2oat`, in-process service calls in
  place of exec, leaving status and nav bar windows out of dumps.

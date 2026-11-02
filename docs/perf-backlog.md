# Performance backlog: 100 zero- to low-risk improvements

Sorted by expected gain. Scope: the six device scenarios of the benchmark (app launch,
navigation, scroll to element, text input, assertion burst, repeated taps).

## Where the time goes now

Measured 2026-11-02 on `emulator-5554` (API 36, host GPU), native Windows build, after the
benchmark's own device reset (agents killed, Settings data cleared, animations back on),
with `VIRTUOSO_DEBUG=1` traces:

| Scenario | Wall | Of which |
|---|---|---|
| App launch | 1.5 s | ~0.54 s process start, agent start and teardown; `am start -W` 0.5–0.6 s; first screen read after launch 0.2–0.26 s |
| Navigation | 5.4 s | 6 settles after tap/back of 0.44–0.77 s each |
| Text input | 3.9 s | type 16 chars 0.33 s (~20 ms a key); erase 20 chars 0.46 s (~23 ms a key); hideKeyboard 0.2 s |
| Repeated taps | ~5 s | 10 settles of 0.34–0.43 s |
| Scroll to element | ~2.6 s | one drag + settle per screenful; tap/back settles |
| Assertion burst | ~1.5 s | launch; then 3–4 ms per assertion |

Fixed costs measured on this host:
- one `adb` invocation costs ~60 ms;
- the agent takes ~200 ms from `app_process` to listening;
- a cached screen read takes 3–25 ms;
- an agent `exec` takes 8–35 ms (the 224 KB `dumpsys package` read takes 21–500 ms).

**Expected gain** is the time saved over one pass of the six scenarios (one run each), in ms,
estimated from those traces:
- An item that touches every run is counted six times.
- Items marked *measure first* rest on a guess about the cause; the trace shows the cost but not yet why.
- Gains overlap where two items attack the same cost (noted), so they do not simply add up.

**Risk:** *Zero* means behaviour cannot change: it is refactoring, fewer round trips, or work moved
off the critical path. *Low* means behaviour could change in a rare case, and the item says how
that case is handled (usually a fallback to today's path).

## Status (2026-11-02)

Each change was made test-first:
- A device or unit test was written and seen to fail, then made to pass.
- For pure refactors, the existing suite was the safety net, run green before and after.
- Gains measured on the emulator are noted where they were measured.

**Done (37):**
1, 2, 3, 4, 7, 8, 9, 10, 11, 13, 15, 16, 17, 18, 19, 20, 22, 23, 24, 26, 27, 29, 30, 32, 34, 35, 37, 40, 41, 55, 69, 70, 81, 82, 87, 95, and 25 (already in place: `write_frame` sends one buffer).

Measured on the same emulator, median of 7 runs, with the benchmark's reset between runs:
- App launch went from 2255 ms to 1330 ms.
- Most of that came from 1, 2, 3, 9, 10 and 11, the agent start and teardown.
- Erasing the whole search field (7) went from 630 ms to about 200 ms; the rest is Settings refreshing its results.

**Measured, no gain, not kept (8):**
- 5: the settle diagnostics (4) show no SystemUI events during a settle.
- 6: the port forward is done before the agent is ready, so nothing overlaps.
- 12: the cold first read still takes 240–310 ms, because the cost is the app building its tree.
- 28: QuickJS starts in 0.8 ms.
- 31 and 33: every settle in the benchmark ends on events, never on the stable-tree path.
- 42 and 43: no measurable start-up gain; process creation dominates.

The diagnostics also show that a settle waits for the app's own last event plus the 80 ms quiet period. Settings sends real changes until 280–450 ms after a tap. That cost stays unless the quiet period or the settle rule changes, and those changes are listed under "left out on purpose" below.

**Superseded (6):**
- 44 and 45: listing devices now takes about 1 ms through the adb server (1).
- 83: superseded by 3.
- 84: superseded by 9.
- 14: superseded by 13, without 14's hidden-API reflection.
- 36: checking a cached launcher activity costs as much as resolving it (8 ms).

**Already true, nothing to change (5):**
- 49: the class name is read once.
- 57 and 79: the no-change wait only applies when no event came, and typing and back always send events.
- 59: a first empty read already returns.
- 61: the `id` pattern is only built when the selector has one.

**Not done (44):**
21, 38, 39, 46, 47, 48, 50–54, 56, 58, 60, 62–68, 71–78, 80, 85, 86, 88–94, 96–100.

Each of these is worth under ~5 ms per pass by its estimate, or zero in the benchmark, except:
- 47 (keep the agent between runs): zero in the benchmark, which kills agents between runs; it needs a version handshake.
- 58 (reuse the screen between assertions): Compose sends no events, so it needs care.
- 48 (skip hints on non-editable nodes): it could change which element a selector matches.
- 78 (a shorter no-change wait after drags): it saves 200 ms only at the end of a list, which the benchmark never reaches, and a list that loads more on scroll may answer late.

## The list

| # | Improvement | Where | Helps | Expected gain | Risk |
|---|---|---|---|---|---|
| 1 | Talk to the adb server directly over its TCP protocol (`host:devices`, `host-serial:…:forward`, `shell:`) instead of spawning `adb` for device listing, agent prep, forwarding and teardown; fall back to `adb` on any protocol error | `android.rs` (`Adb::run`, `devices`, `AndroidAgent::start`, `Drop`) | all six | ~1500 (5 spawns × ~55 ms × 6) | Low |
| 2 | At exit, tell the agent to quit over its socket and do not wait: drop the `STOP_AGENT` wait loop (≥100 ms of `sleep 0.1`) and its `adb shell` from `Drop` (the next start kills a stale agent anyway) | `android.rs:21`, `android.rs` `impl Drop for AndroidAgent`; new `quit` op in `Server.java` | all six | ~1000 (~170 ms × 6) | Low |
| 3 | Put the animation check-and-disable into the agent prep shell, which already runs before the agent starts, instead of two agent `exec`s after it (82 + 90 ms when the reset turned animations back on) | `android_device.rs:73` `disable_animations`, `main.rs:814`, `android.rs:178` prep | all six | ~1000 (~170 ms × 6) | Zero |
| 4 | Find what keeps settles going after a tap or back (10–12 events over 0.35–0.75 s): log the event type and package per settle under `VIRTUOSO_DEBUG` | `Settle.java` `onEvent`, `waitSettled` | enabler for 5, 31, 33 | 0 (diagnostic) | Zero |
| 5 | Do not let SystemUI content changes (status bar clock, battery, signal icons) extend a settle for an action on another app's window; window-state changes still count, so dialogs and the shade still do | `Settle.java` `onEvent` | navigation, taps, input, scroll | ~1000 expected, range 0–3000 (21 settles); *measure first* with 4 | Low |
| 6 | Mark the agent listening before `UiAutomation.connect()` finishes: bind the socket and print `listening` first, connect UiAutomation on its own thread, and hold the first request until it is ready, so host forwarding and connecting overlap the connect | `Main.java` `main`, `connect` | all six | ~500 (~80 ms × 6); *measure first* | Low |
| 7 | `eraseText` covering the whole field: clear it with one `ACTION_SET_TEXT ""` on the focused editable node instead of N synchronous DEL keys; any other case keeps the keys | `Input.java:204` `erase` | text input | ~440 (the benchmark erases 20 of 16 chars) | Low |
| 8 | Read ungranted permissions without the 224 KB `dumpsys package`: `PackageManager.getPackageInfo(GET_PERMISSIONS)` + `checkPermission` in the agent (or filter with `grep` on the device); keep dumpsys as the fallback | `android_device.rs:198` `grant_all`; `android.rs` `ungranted_runtime_permissions` | all six (every flow launches) | ~360 (21–34 ms warm, up to 500 ms cold, × 6) | Low |
| 9 | One `adb shell` for prep and agent launch: run the prep, then `exec app_process` in the same shell (one round trip fewer) | `android.rs:178–200` | all six | ~360 (60 ms × 6) | Zero |
| 10 | Create the `adb forward` while the agent boots instead of after `listening` (forwarding needs no listener) | `android.rs:220` | all six | ~360 (60 ms × 6) | Zero |
| 11 | Use a fixed local port per serial (`tcp:<hash(serial)>`, rebound each run) instead of `tcp:0`, so no `forward --remove` is needed at exit; on a bind clash fall back to `tcp:0` | `android.rs:220`, `Drop` | all six | ~360 (60 ms × 6) | Low |
| 12 | Ask for the whole tree in one fetch on API 33+: `getRoot(FLAG_PREFETCH_DESCENDANTS_HYBRID or DEPTH_FIRST)` for each window, to cut IPCs on cold reads (the first read after launch takes 200–260 ms) | `Screen.java:59,73` | all six (first read), navigation | ~300 (~50 ms × 6); *measure first* | Low |
| 13 | Run `force-stop`, permission grants, `mark` and `am start -W` in one agent `exec` (`sh -c` script, names already checked to be plain) instead of 3–4 `exec`s | `android_device.rs` `launch_app` | all six | ~240 | Zero |
| 14 | Start activities, force-stop and grant permissions through the system services in the agent (IActivityTaskManager / IActivityManager / IPackageManager by reflection), with `exec` as the fallback per call | `android_device.rs` `launch_app`; new agent ops | all six | ~240 (overlaps 13) | Low |
| 15 | `wait_for_app`: one screen read per poll; today each poll reads twice (`raw_screen` and then `screen()` for error dialogs) | `android_device.rs:176` | all six | ~180 | Zero |
| 16 | Native Windows only: raise the timer resolution while running (`timeBeginPeriod(1)`), so `sleep(20 ms)` polls do not round up to ~31 ms | `main.rs` start-up | all six (native) | ~200 native, 0 in WSL | Zero |
| 17 | `hideKeyboard`: skip the runner's extra `settle()` after the device has already confirmed the keyboard window is gone | `runner.rs` `Cmd::HideKeyboard`, `android_device.rs:345` | text input | ~100 | Low |
| 18 | `scrollUntilVisible`: drag 55% of the screen per step (from 75% to 20%) instead of 40% (from 70% to 30%), keeping a 45% overlap so nothing is skipped | `runner.rs:918–921` | scroll | ~150 (fewer drags + settles on long lists) | Low |
| 19 | A `windows` op that returns only window info, for `wait_for_app` and `keyboard_shown`, instead of a full node dump | `android_device.rs:176,193`; `Server.java`, `Screen.java` | all six, input | ~120 | Zero |
| 20 | Screen dump: call `getRoot()` once per window; today it is called in the windows loop and again in the nodes loop | `Screen.java:59,73` | all six | ~100 (~0.5 ms × ~180 reads) | Zero |
| 21 | Deserialize the screen reply straight into structs (`#[derive(Deserialize)]` nodes) instead of through `serde_json::Value` | `screen.rs` `from_agent`, `android_device.rs:160` | all six | ~90 (~0.5 ms × ~180 reads) | Zero |
| 22 | Keep compiled selector patterns across polls of one lookup (today `Pattern::new` builds a fancy-regex on every poll) | `select.rs:42`, `runner.rs:751` | all six | ~80 | Zero |
| 23 | Combined `tap+settle`, `key+settle` and `swipe+settle` agent ops: one round trip instead of two after each action | `android_device.rs` `tap`/`press_key`/`swipe`/`settle`; `Server.java` | navigation, taps, scroll, input | ~80 | Zero |
| 24 | `find_within` / `wait_gone`: poll every 10 ms instead of 20 ms while waiting for an element | `runner.rs:747,762` | navigation, taps, assertions | ~75 | Zero |
| 25 | Send each frame in one write (length and body together); today two `send`s with `TCP_NODELAY` go out as two packets | `agent.rs:16` `write_frame` | all six | ~60 (~0.1 ms × ~600 calls) | Zero |
| 26 | `wait_for_app`: poll every 10 ms instead of 30 ms | `android_device.rs:176` | all six | ~60 | Zero |
| 27 | Wrap the agent socket in a `BufReader` on the host (one `recv` per frame instead of header and body separately) | `agent.rs:26` `read_frame` | all six | ~30 | Zero |
| 28 | Create the QuickJS runtime only when a flow first needs JavaScript (`${…}`, scripts, conditions) | `runner.rs` `Runner::new`, `js.rs` | all six | ~30 (~5 ms × 6) | Zero |
| 29 | `awaitInputFocus`: poll every 5 ms instead of 20 ms | `Input.java:190` | text input | ~30 | Zero |
| 30 | Have the agent's `listening` line carry width, height, rotation and SDK, and skip the separate `info` call | `Main.java`, `android_device.rs` `refresh_info` | all six | ~18 | Zero |
| 31 | Settle: begin the stable-tree check at 200 ms instead of 300 ms (it only decides on screens whose events never stop) | `Settle.java:29` | navigation, taps (screens with endless animation) | 0–100; *measure first* with 4 | Low |
| 32 | Skip the `ping` after connecting; the first real request proves the agent answers | `android.rs` `start` | all six | ~12 | Zero |
| 33 | Settle: when the event that started a change is `TYPE_WINDOW_STATE_CHANGED` for the target app and the idle probe passes, stop at the end of the quiet period instead of rechecking stability | `Settle.java` `waitSettled` | navigation, taps | 0–300; *measure first* with 4 | Low |
| 34 | `hide_keyboard`: poll every 10 ms instead of 30 ms | `android_device.rs:345` | text input | ~10 | Zero |
| 35 | Skip `am force-stop` when the app has no process (`pidof`), folded into the launch script of 13 | `android_device.rs` `launch_app` | all six | ~40 (overlaps 13) | Zero |
| 36 | Cache the launcher activity on disk (per serial, package and `lastUpdateTime`) so later runs skip `resolve-activity` | `android_device.rs:164` | all six | ~48 (8 ms × 6) | Low |
| 37 | Fold the `mark` before `am start` into the `exec` request (a `mark: true` flag) | `android_device.rs` `launch_app`; `Server.java` | all six | ~6 | Zero |
| 38 | Precompute each element's lowercase label once per screen tree instead of in every `Pattern::matches` | `select.rs:33`, `screen.rs` | all six | ~20 | Zero |
| 39 | Precompute `matchable()` once per screen tree (a bitset) instead of per element per selector | `screen.rs:146`, `select.rs` | all six | ~15 | Zero |
| 40 | `error_dialog_button`: test `package == "android"` before lowercasing each node's text | `android_device.rs` `error_dialog_button` | all six | ~10 | Zero |
| 41 | Give the agent's `Json.Out` a 16 KB starting buffer (it starts at 4 KB, so a 12.5 KB dump grows it twice) | `Json.java:107` | all six | ~2 | Zero |
| 42 | Release profile: `lto = "fat"`, `codegen-units = 1` | `Cargo.toml` | all six | ~10 | Zero |
| 43 | Windows: link the C runtime statically (`+crt-static`), so fewer DLLs load at process start | `.cargo/config.toml` | all six (native) | ~10 | Zero |
| 44 | When `--device` (or `ANDROID_SERIAL`) names a device, skip `adb devices` and let the first `adb -s` call report a missing device | `main.rs` `pick_devices` | all six (with a serial) | ~360 when a serial is given; 0 in the benchmark | Low |
| 45 | List devices while the flows are parsed (two threads), not after | `main.rs` `test` | all six | ~10 | Zero |
| 46 | Build step names with `describe()` without `format!("{:?}")` of the whole command (Debug-printing a `repeat` prints its whole subtree) | `runner.rs:1068` | taps (repeat) | ~5 | Zero |
| 47 | Keep the agent alive between `virtuoso` runs (its token kept in a per-serial state file, jar hash checked) and reuse it when it answers | `android.rs` `start` | all six | 0 in the benchmark (its reset kills agents); ~400 a run in daily use | Low |
| 48 | Screen dump: skip `getHintText` / `getError` calls on nodes that are not editable | `Screen.java` `walk` | all six | ~20 | Zero |
| 49 | Screen dump: read `getClassName()` once per node (it is converted twice today, for `cls` and the Compose check) | `Screen.java` `walk` | all six | ~5 | Zero |
| 50 | Signature reads for the restless check: one `StringBuilder` per node instead of string concatenation | `Screen.java` `sign` | navigation, taps (restless screens) | ~5 | Zero |
| 51 | `Restless.stableAfter`: compare against the previous read with arrays indexed by node path instead of `HashMap`/`HashSet` rebuilt each read | `Settle.java` `Restless` | navigation, taps (restless screens) | ~5 | Zero |
| 52 | `idle()` probe: reuse the root from the last probe when the active window has not changed | `Settle.java` `idle` | all six | ~10 | Low |
| 53 | Return a typing error without `awaitInputFocus` when the focused editable node was already found by the tap that preceded it (pass the node id) | `Input.java` `type` | text input | ~5 | Low |
| 54 | `insert()` fallback: look up the focused node once per run of non-key characters, not once per character | `Input.java` `insert` | text input (non-ASCII) | 0 in the benchmark | Zero |
| 55 | `KeyCharacterMap.load` once per agent, not once per `type` | `Input.java` `type` | text input | ~4 | Zero |
| 56 | `stamped()`: restamp events with one clock read per character, not per event | `Input.java` `stamped` | text input | ~2 | Zero |
| 57 | After `inputText`, settle with the 100 ms no-change wait only when the field produced no text-changed event | `runner.rs:665` `settle_typed` | text input | ~30 | Low |
| 58 | `assertVisible` right after a lookup that already read the same screen: reuse the tree when no action was sent and the agent reports no event since (only when the last dump showed no Compose, which sends no events) | `runner.rs` `find`, `android_device.rs`, agent event counter | assertion burst | ~45 (3 ms × 15) | Low |
| 59 | `assertNotVisible` with nothing matching on the first read: return without a second read | `runner.rs:736` `wait_gone` | assertion burst | ~3 | Zero |
| 60 | Relative selectors (`below:`): evaluate the anchor once per screen tree, not once per candidate | `select.rs` | assertion burst | ~2 | Zero |
| 61 | Compile a selector's `id` pattern only when the selector has one (today an empty pattern is built too) | `select.rs:44` | all six | ~2 | Zero |
| 62 | `expand_selector`: skip cloning selectors that contain no `${` | `runner.rs` `expand_selector` | all six | ~2 | Zero |
| 63 | Step callback output: buffer the live view's lines and flush once per step | `main.rs` `Ui` | all six | ~5 | Zero |
| 64 | Write the JUnit/JSON report from the in-memory result without re-walking the steps (only with `--format`) | `report.rs` | none in the benchmark (`noop`) | 0 | Zero |
| 65 | `collect()`: skip reading `config.yaml` when the path is a single flow file | `workspace.rs` | all six | ~1 | Zero |
| 66 | Cache parsed sub-flows within a run: a `repeat` around `runFlow: file` reparses the file every iteration | `runner.rs` `run_file` | none in the benchmark | 0 | Zero |
| 67 | Run the agent's server thread at a higher priority (`Process.setThreadPriority(THREAD_PRIORITY_DISPLAY)` where the shell user may) | `Main.java` | all six, on a loaded emulator | ~20; *measure first* | Low |
| 68 | Lower the agent's GC churn: reuse one `Rect` and one `StringBuilder` across nodes (the `Rect` is shared already, the builders are not) | `Screen.java` | all six | ~5 | Zero |
| 69 | Drop the `layer` field from each window in the dump; the host never reads it | `Screen.java` `dump` | all six | ~1 | Zero |
| 70 | Shorter JSON: omit `"c":""`, `"i":""`, `"t":""` when empty (the host already defaults them) | `Screen.java` `walk`, `screen.rs` | all six | ~10 (~30% smaller dumps) | Zero |
| 71 | Omit `"k"` (class) for nodes whose class repeats their parent's; or send class names as indexes into a table | `Screen.java`, `screen.rs` | all six | ~5 | Low |
| 72 | Host: reuse one `String` buffer for request JSON (`serde_json::to_writer` into a kept `Vec`) | `agent.rs` | all six | ~1 | Zero |
| 73 | `launch_app`: build the `am start` argv without the `Vec<String>`/`Vec<&str>` double allocation | `android_device.rs` | all six | <1 | Zero |
| 74 | Skip the `rotation` refresh after actions that cannot rotate (it is only needed after `setOrientation`) | `android_device.rs` | none in the benchmark | 0 | Zero |
| 75 | `swipe` for scrolling: 250 ms backdated drags instead of 300 ms (same distance; no real-time cost, but fewer move events to inject) | `runner.rs:924` | scroll | ~2 | Low |
| 76 | `Input.swipe`: inject the move events with fewer steps (16 ms apart instead of 8 ms, still above 60 fps) | `Input.java:25` | scroll | ~5 | Low |
| 77 | `scrollUntilVisible`: when the element matches but is under 100% visible at the screen edge, drag only the missing part instead of a full step | `runner.rs` `scroll_until_visible` | scroll | ~100 when it applies; *measure first* | Low |
| 78 | `scrollUntilVisible`: settle with the 100 ms no-change wait after a drag that produced scroll events (the list moved), keep 300 ms when it did not (end of list) | `runner.rs:924–925` | scroll | ~50 | Low |
| 79 | `back`: settle with no no-change floor when Back produced a window-state change (the screen closed) | `runner.rs` `Cmd::Back` | navigation, taps | ~20 | Low |
| 80 | `tapOn` on an element whose screen is already settled: skip the `mark` round trip (the tap op marks) | `android_device.rs` | navigation, taps | ~2 | Zero |
| 81 | Agent `exec`: read with a 64 KB chunk into a presized buffer (8 KB chunks today; the 224 KB `dumpsys` grows the buffer many times) | `Exec.java` | all six (launch) | ~5 | Zero |
| 82 | Agent `exec`: decode the output with `buf.toString(UTF_8)` instead of `new String(buf.toByteArray(), …)` (one copy fewer) | `Exec.java` | all six (launch) | ~1 | Zero |
| 83 | `settings get` for the animation check: read all three values with `settings list global` in one call (if 3 is not done) | `android_device.rs:73` | all six | ~30 (overlaps 3) | Zero |
| 84 | Push the agent jar with `adb push --sync` (skips the transfer when sizes and times match, without the hash-named check shell) | `android.rs:178–188` | first run after a build | ~100 once | Low |
| 85 | Keep `d8` output small: build the agent with `--release` and `--min-api 26` so ART verifies fewer methods at start-up | `build.rs` | all six | ~10; *measure first* | Zero |
| 86 | Agent start: avoid the `Class.forName` reflection for input injection until the first input op | `Input.java` constructor | all six | ~5 | Zero |
| 87 | `ScreenTree::from_agent`: look up each window's package once per window; today a `String` is built for every node | `screen.rs:134` | all six | ~10 | Zero |
| 88 | Host start: avoid `std::env::current_exe` and similar lookups on the `test` path | `main.rs` | all six | <1 | Zero |
| 89 | Host start: parse only the needed clap subcommand (`clap` derive with lazy subcommand construction is already default; drop unused features such as `suggestions`) | `Cargo.toml` (clap features) | all six | ~2 | Zero |
| 90 | Host start: build `regex`/`fancy-regex` without the Unicode tables the selectors never need (default features trimmed) | `Cargo.toml` | all six | ~2 | Low |
| 91 | `ScreenTree::new`: reserve the parent stack once and reuse it | `screen.rs:89` | all six | <1 | Zero |
| 92 | `fingerprint()` hashing: hash bounds and text without formatting strings | `screen.rs:167` | navigation, taps | ~2 | Zero |
| 93 | Trace off: avoid cloning the request JSON in `call` when tracing is off (already returns early; the restart path clones every request) | `android_device.rs` `call` | all six | ~2 | Zero |
| 94 | Restart path: clone the request only after a failure (keep the serialized bytes instead) | `android_device.rs` `call` | all six | ~2 | Zero |
| 95 | Secret masking: skip `mask()` when no variables were set (no `-e`, no `env`) | `runner.rs` `mask` | all six | ~1 | Zero |
| 96 | Step timing: use one `Instant` per step instead of several | `runner.rs` `run_steps` | all six | <1 | Zero |
| 97 | `on_step` callback: pass `&StepResult` without cloning the error string | `runner.rs` | all six | <1 | Zero |
| 98 | Agent server: skip `Json.parse` of the hello frame's unused fields | `Server.java:59` | all six | <1 | Zero |
| 99 | Agent server: reply with a constant frame for `ping` (no JSON builder) | `Server.java` | all six | <1 | Zero |
| 100 | Benchmark hygiene, so gains show: record each settle's `waited` and `method` in a `--profile` JSON next to the step results, so a regression in any of the above is visible per command | `runner.rs`, `main.rs` | all six | 0 (measurement) | Zero |

## Bigger gains left out on purpose (not low risk)

These would save more than most of the list. Each one changes when virtuoso acts, so a flow
could behave differently:

- **Lazy settle:** after a tap, move on as soon as the next command's element appears, instead of waiting for the screen to settle. This could save about 3–6 s per pass, but a tap could land mid-transition.
- **Shorter quiet period:** settle after 50 ms of quiet instead of 80 ms. This could save about 0.6 s over 21 settles, but slow apps would be read early.
- **Faster typing and erasing:** send typed characters, or DEL keys, asynchronously in chunks. Characters were lost to IME composing before (the reason `TYPE_CHUNK` is 1); text input could gain about 0.5 s.
- **Fewer accessibility nodes:** drop `FLAG_INCLUDE_NOT_IMPORTANT_VIEWS`, giving smaller and faster dumps. Selectors would no longer see some nodes that Maestro matches.
- **Overlapped launch:** start the app's launch while the agent is still booting, saving about 0.25 s per launch. This reorders the flow's `onFlowStart` hooks.

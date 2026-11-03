# Install size backlog

Sorted by expected gain. Install size is what the benchmark counts: the size on disk of the
`virtuoso` binary, with the agent jar embedded in it. speedrun is 3,636,152 B (3.47 MiB), a
dynamically linked glibc binary, so its libc is not counted.

## Status (2026-11-03)

Done, test first. `crates/virtuoso/tests/size.rs` holds a budget for each OS: before each item
the budget was set one byte under the current size (red), and the item had to shrink the binary
to pass (green). The Linux checks run in WSL against the installed binary from
`bench/build.sh`. Every item that could change behaviour also has its own test (regex
features, the argument parser, script built-ins, PNG round trip, error wording, base64, the
small sort, and the agent's size budget). All suites pass, on the device too, and clippy is
clean.

| Build | Before | After |
|---|---|---|
| Windows | 4,190,208 B | **2,223,104 B** (−47%) |
| Linux (static) | 5,883,888 B (glibc) | **2,229,392 B** (musl, −62%) |
| speedrun, for comparison | | 3,636,152 B |

Windows, each item measured on top of the ones before (bytes):

| # | Item | Saving |
|---|---|---|
| 2 | Everything at `opt-level = "z"` (3: all at "s" −441 KB, 4: deps at "z" −240 KB, 5: deps at "s" −212 KB, measured and not used) | −565,760 |
| 6, 35 | fancy-regex 0.19 with the regex features selectors use | −610,816 |
| 11 | Hand-written argument parser instead of clap (covers 15 and 16) | −337,920 |
| 10 | QuickJS at `opt-level = "s"` | −247,296 |
| 18 | One-line value printer for flow errors instead of the YAML emitter | −41,472 |
| 20 | Flows shared through `Arc`, not cloned | −32,768 |
| 19 | Command names without Debug formatting | −32,256 |
| 23 | Glob matcher instead of the `regex` crate | −25,600 |
| 22 | PNG written with stored deflate blocks | −19,456 |
| 24 | Small stable sort for workspace paths | −11,776 |
| 17 | Only the JavaScript built-ins flows use | −7,680 |
| 12 | Loopback addresses without a name resolver | −6,656 |
| 31 | Small sort for ordered flows | −5,120 |
| 32 | Small sort for secrets | −5,120 |
| 21 | One request builder instead of `json!` literals | −4,608 |
| 26 | Compact JSON reports (one formatter) | −3,072 |
| 36 | base64 written here, crate dropped | −2,560 |
| 27 | Unstable sort for selector matches | −2,048 |
| 28 | Unstable sort for latency samples | −2,048 |
| 33 | Agent shrunk by R8 (jar 16,932 → 14,644 B) | −2,048 |
| 29 | `{:?}` formatting replaced (10 sites) | −512 |
| 37 | Minified report CSS (113 B of text; crossed an alignment boundary) | −512 |
| 30 | Minified JS prelude: 550 B less text, absorbed by section padding | 0, reverted |
| 38 | rquickjs `classes`/`properties` features dropped | 0, reverted |

Linux, measured in WSL in this order (bytes):

| # | Item | Size | Saving |
|---|---|---|---|
| | All the code changes above, glibc static as before | 3,683,744 | −2,200,144 |
| 1 | Static musl | 2,548,424 | −1,135,320 |
| 13 | QuickJS without unwind tables | 2,470,600 | −77,824 |
| 14 | lld with identical-code folding | 2,457,928 | −12,672 |
| 9 | `.eh_frame` and `.eh_frame_hdr` removed after linking | 2,232,496 | −225,432 |
| 25 | No build id, notes or comments | 2,232,080 | −416 |
| 34 | Build paths taken out of panic messages | 2,229,392 | −2,688 |

Changes a user can see: help text and argument errors come from virtuoso's own parser; flow
error messages show values inline (`[a, b]`); `\p{…}` classes in selectors match as plain
text; scripts no longer have Proxy, typed arrays, BigInt or WeakRef; JSON reports are
written compact; bounds print as `[0,10][100,50]`.

## Where the bytes go

Baselines (2026-11-02): Windows `virtuoso.exe` 4,190,208 B. Linux, statically linked glibc as
`bench/build.sh` builds it, 5,883,888 B. Measured with cargo-bloat (`.text`) and section sizes:

| Area | Windows | Linux |
|---|---|---|
| Regex stack (regex-automata, regex-syntax, aho-corasick, fancy-regex, memchr) and its Unicode tables | 743 KB | 770 KB |
| QuickJS (C) | 610 KB | ~630 KB |
| std | 619 KB | 663 KB |
| virtuoso's own code | 482 KB | 480 KB |
| clap | 314 KB | 303 KB |
| serde_yaml + unsafe-libyaml | 136 KB | 134 KB |
| serde_json | 84 KB | 87 KB |
| png + deflate | 73 KB | 81 KB |
| std sort instantiations | 74 KB | 94 KB |
| Static glibc: DNS/NSS/dlopen pulled in by one `getaddrinfo` | — | 223 KB |
| Static glibc: scanf/printf/strto* | — | 192 KB |
| Backtrace symbolisation (gimli/addr2line) | — | 166 KB |
| `.eh_frame` + `.eh_frame_hdr` | — | 372 KB |

The static glibc build costs about 1.1 MB in this metric. A dynamic build is 4,746,696 B, but
static linking was chosen because it saves ~1.1 MB of resident memory.

## The list

1 KB = 1024 B. "(m)" is measured on a variant build; "(g)" is a guess. "Alt" items are
alternatives to the item named, not additions. Items that change speed need a benchmark run
as well as their test.

| # | Improvement | Where | Linux KB | Windows KB | Risk | Red/green |
|---|---|---|---|---|---|---|
| 1 | Static **musl** instead of static glibc for the Linux release | `bench/build.sh` (`--target x86_64-unknown-linux-musl`; install musl-tools) | **−1229 (m)** | — | Low–medium: musl's malloc may be slower (bench CPU and timings); RSS fell (lint 4.6 → 2.7–3.0 MB) | Size-budget test (below); full bench |
| 2 | `opt-level = "z"` for everything | `Cargo.toml` `[profile.release]` | −1136 (m) | −1110 (m) | Functionally zero; may slow selectors and JSON (bench) | Bench timings |
| 3 | Alt to 2: `opt-level = "s"` for everything | same | −1024 (m) | −933 (m) | As 2 | Bench timings |
| 4 | Dependencies at `"z"`, virtuoso's own code at 3 | `[profile.release.package."*"] opt-level = "z"` | −765 (m) | −704 (m) | Low: the hot code is ours and the regex engine | Bench timings |
| 5 | Alt to 4: dependencies at `"s"` | same | −727 (m) | −622 (m) | Low | Bench timings |
| 6 | Trim regex features: fancy-regex `default-features = false, features = ["std"]`, regex-automata with only `unicode-case`, `unicode-perl`, `unicode-word-boundary` | `crates/virtuoso/Cargo.toml` | −612 (m) | −595 (m) | Low: `\b`, `\w`, `(?i)` on accented text, lookarounds and backreferences keep working; `\p{…}` classes stop compiling and fall back to a literal match (`unicode-gencat` keeps `\p{L}` for +55 KB). **`unicode-word-boundary` is required:** without it any `\b` pattern panics in fancy-regex | Select tests for `\b`, `(?i)Ä`, `\w` with accents, lookbehind, backreference, `\p{L}` |
| 7 | Alt to 6: drop only regex `perf` (keep all Unicode) | same | −376 (m) | −358 (m) | Zero–low: short anchored patterns gain nothing from prefilters | Select tests; bench |
| 8 | Alt to 6: trim Unicode but keep `perf` | same | −273 (m) | −273 (m) | Low | As 6 |
| 9 | Strip `.eh_frame`/`.eh_frame_hdr` from the Linux binary after linking | `bench/build.sh` (`objcopy --remove-section …`) | −360 (m) | — | Low–medium: no backtraces; check a forced panic still exits cleanly | Panic smoke test |
| 10 | QuickJS C at `opt-level = "s"` | `[profile.release.package.rquickjs-sys]` | −336 (m) | −241 (m) | Low: scripts are not hot | `tests/js.rs`; a script-heavy flow's time |
| 11 | Replace clap with a hand-written parser (lexopt or pico-args) | `main.rs:21-176` (33 options, ~10 subcommands) | −240…−280 (g) | −250…−290 (g) | Low–medium: help text, aliases, comma lists and global options must be redone | `tests/cli.rs` plus `--help` snapshot tests |
| 12 | Build loopback addresses as `SocketAddr` instead of resolving `"127.0.0.1"` (drops glibc's `getaddrinfo`, NSS and dlopen) | `main.rs:281`, `main.rs:344` | −143 (m) | −6.5 (m) | Zero | Test that the Linux release binary has no `getaddrinfo` |
| 13 | Compile QuickJS without unwind tables (`-fno-asynchronous-unwind-tables -fno-unwind-tables`) | `CFLAGS_x86_64_unknown_linux_gnu` in `bench/build.sh` | −85 (m) | — | Low: panic=abort, nothing unwinds through C | `tests/js.rs` |
| 14 | Identical-code folding on Linux (`-C link-arg=-Wl,--icf=all`, rust-lld) | `bench/build.sh` RUSTFLAGS | −71 (m) | already on | Low: nothing relies on function-pointer identity | Full test suite |
| 15 | clap with only `std`, `derive`, `help` | `crates/virtuoso/Cargo.toml` | −63 (m) | −82 (m) | Low–medium: terser usage errors | CLI tests |
| 16 | Alt to 15: clap without `color` and `suggestions` | same | −22 (m) | −42 (m) | Zero–low: no "did you mean" | CLI tests |
| 17 | QuickJS with only the intrinsics flows use (Date, Eval, RegExp, JSON, Map/Set, Promise) | `js.rs` `Context::full` → `Context::custom` | −68 (m) | −15.5 (m) | Low: scripts using Proxy, typed arrays, BigInt or WeakRef break | JS test of what stays; document what goes |
| 18 | A small value printer for flow error messages instead of the YAML emitter | `flow.rs:690` `show()` | −43 (m) | −40 (m) | Low: error wording changes | Flow parse error-message tests |
| 19 | Command names in `describe` by match, not `format!("{other:?}")` | `runner.rs:1076-1079` | −33 (m) | −32 (m) | Zero | Test `describe` for every command |
| 20 | Share flows instead of cloning them (`Arc<Flow>` or indices) | `main.rs:791,793` | −25…−37 (g) | −25…−33 (g) | Zero | Existing sharding tests; bloat check |
| 21 | Typed request structs (or one small writer) instead of 53 `json!` request literals | `android_device.rs`, `agent.rs` | −10…−20 (g) | −10…−20 (g) | Zero | Agent protocol and device tests |
| 22 | PNG out with stored (uncompressed) deflate blocks: drops the second miniz_oxide | `image_util.rs` | −10…−20 (g) | −10…−20 (g) | Low: cropped PNGs are larger on disk | Crop tests (pixel-equal) |
| 23 | Drop the direct `regex` dependency: globs through fancy-regex or a ~30-line matcher | `workspace.rs:11,58-79,135`, `Cargo.toml:21` | −18 (m) | −15 (m) | Zero | `tests/workspace.rs` (passed in the survey) |
| 24 | Sort workspace paths with `sort_by_cached_key` (one 13 KB instantiation) | `workspace.rs:138` | −5…−10 (g) | −5…−10 (g) | Zero | Execution-order tests |
| 25 | Strip `.note.stapsdt`, `.comment` and the build id | `bench/build.sh` | −7.3 (m) | — | Zero | Size test |
| 26 | One JSON writer for reports (pretty and compact are both instantiated) | `report.rs:159,207` | −5…−10 (g) | −5…−10 (g) | Low: report file layout | Report tests |
| 27 | Unstable sort where keys are unique | `select.rs:123` | −3…−5 (g) | −3…−5 (g) | Zero | Select order tests |
| 28 | Unstable sort for latency samples | `latency.rs:16` | −3…−5 (g) | −3…−5 (g) | Zero | Latency tests |
| 29 | Remove `{:?}` formatting of strings across the crate (13 sites; the string Debug escaper then goes) | `adb_wire.rs:102,121`, `android.rs:309`, `android_device.rs:397`, `envfile.rs:12`, `js.rs:113`, `main.rs:298,870`, `mcp.rs:154`, `image_util.rs:23` | −3…−6 (g) | −3…−6 (g) | Zero: messages quote with Display | Error-message tests |
| 30 | Minify the embedded JS prelude (5.3 KB) | `prelude.js`, at build time | −2…−3 (g) | −2…−3 (g) | Low: error line numbers in the prelude change | JS tests |
| 31 | Sort the ordered flows by index, not by `(usize, Flow)` | `workspace.rs:155` | −2…−4 (g) | −2…−4 (g) | Zero | Execution-order tests |
| 32 | Sort secrets by length without a `(String, String)` sort instantiation | `runner.rs:165` | −2…−4 (g) | −2…−4 (g) | Zero | Masking tests |
| 33 | Shrink the agent with R8 instead of plain d8 | `build.rs:74` | −3…−6 (g) | −3…−6 (g) | Low: reflection in the agent needs keep rules | Device agent tests |
| 34 | `--remap-path-prefix` for registry paths in panic messages | `bench/build.sh` RUSTFLAGS | −3.8 (m) | ~−3 (g) | Zero | Size test |
| 35 | fancy-regex 0.14 → 0.19 | `Cargo.toml` | −1.5 (m) | −1.5 (m) | Low | Select tests |
| 36 | Drop the base64 crate (one use, the MCP screenshot) | `mcp.rs:68` | −1.3 (m) | −1.4 (m) | Zero | MCP screenshot test |
| 37 | Minify the HTML report's CSS | `report.rs` | ~−1 (g) | ~−1 (g) | Zero | Report tests |
| 38 | Drop rquickjs `classes` and `properties` features (no code uses them) | `crates/virtuoso/Cargo.toml:25` | 0…−2 (g) | 0…−2 (g) | Zero | Build and JS tests |

### Measured combinations

| Combination | Linux | Windows |
|---|---|---|
| **A**: 6 + 10 + 12 + 16 + 19 | 4,711,296 B (−1145 KB) | 3,249,152 B (−919 KB) |
| A + 13 + 14 | 4,574,528 B (−1279 KB) | — |
| A + 5 (dependencies at "s") | 4,431,296 B (−1418 KB) | 2,979,840 B (−1182 KB) |
| A + 2 (everything at "z") | 4,037,152 B (−1803 KB) | 2,501,120 B (−1650 KB) |
| A + 13 + 14 + 2 | 3,925,728 B (−1912 KB) | — |
| **A on musl** (with 13; speed unchanged, opt-level 3) | **3,474,216 B**, under speedrun | — |

All 242 non-device tests passed on item 6 alone. The survey did not run tests on the
combinations, and none of them has been through the device tests or the benchmark yet.

The cheapest route to a smaller binary than speedrun's, without trading speed: combination A
plus musl (item 1). That gives 3.47 MB on Linux, and 3.25 MB on Windows from combination A
alone. Opt-level changes (items 2–5) go further but need a benchmark run to show what they
cost in speed.

### A red/green guard for every item

Add a test that runs only in release builds (`#[cfg(not(debug_assertions))]`). It asserts
that `metadata(env!("CARGO_BIN_EXE_virtuoso")).len()` is under a budget for the OS, and it
runs with `cargo test --release`. Each item lowers the budget, so the test fails first and
passes once the change is in. Item 12 also gets a check that the Linux binary contains no
`getaddrinfo`.

## Tried, no gain

- `-C force-unwind-tables=no`: 0 on Linux; std, glibc and the C code carry their own tables.
- `-Wl,-z,noseparate-code`: 0.
- `/OPT:ICF` on Windows: already the default.
- png 0.18: +19 KB.
- PNG `Compression::Fast`: 0; both encoder paths stay linked.
- QuickJS at `"z"` instead of `"s"`: identical.
- `opt-level = "s"` for virtuoso's own code only: −297 KB on Windows but +402 KB on Linux.
- Recompressing the agent jar: it is already deflated (31 KB of dex to 16.8 KB).
- `d8 --release`: already used.

## Left out (not low risk)

- **QuickJS's own regex engine (already linked) instead of fancy-regex:** up to −1.3 MB, the
  largest lever left. Its dialect differs, though (no inline flags, possessive quantifiers,
  `\A` or `\z`), so selectors could match differently. Medium risk.
- **UPX compression:** more resident memory (no page sharing), slower start, and antivirus
  false positives on Windows.
- **Nightly `-Z build-std` with `panic_immediate_abort`, `-Zfmt-debug=none` and
  `-Zlocation-detail=none`:** would also drop the 166 KB backtrace code, but needs a nightly
  toolchain.
- **Dynamic glibc:** −1.1 MB, but gives back the ~1.1 MB of resident memory that static
  linking saved.
- **QuickJS without Map, Set or Promise:** −24 KB on Windows and −114 KB on Linux, but valid
  scripts break.
- **Windows `/FIXED`:** −22 KB, but loses ASLR.
- **`-C target-cpu`:** the binary would not run on older CPUs.
- **Another YAML parser:** the small ones are not much smaller, and YAML edge cases are a risk.

Beyond the 38 items above, the remaining ideas each save under 1 KB, so they are not listed.

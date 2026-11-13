//! The command line: `virtuoso test` takes `maestro test`'s arguments. Parsed by hand from a
//! table of commands and options, which also writes the help (clap was ~300 KB of the binary).
//!
//! Options take `--name value` or `--name=value` (`-e value` or `-evalue` for short ones),
//! global options go before or after the command, and `--` ends the options.

use std::fmt::Write;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Cli {
    /// Device serial(s), comma-separated (default: the first adb device).
    pub device: Option<String>,
    /// With no device connected, start the first emulator (AVD) and wait for it to boot.
    pub auto_start_emulator: bool,
    /// With no device connected, start this AVD.
    pub start_emulator: Option<String>,
    /// Only `android` is supported.
    pub platform: String,
    pub command: Command,
}

#[derive(Debug)]
pub enum Command {
    Test(Box<TestArgs>),
    Lint(LintArgs),
    Devices,
    Mcp,
    Screenshot { path: Option<PathBuf> },
    Skills { dir: Option<PathBuf>, print: bool },
    Doctor,
    Session(SessionCmd),
    Look { all: bool },
    Do { commands: Vec<String> },
    Latency { count: usize },
    Hierarchy { all: bool },
}

#[derive(Debug)]
pub enum SessionCmd {
    Start { app: String },
    Log,
    Save { path: PathBuf },
    Clear,
    Stop,
    Serve { app: String },
}

#[derive(Debug)]
pub struct LintArgs {
    pub paths: Vec<PathBuf>,
    pub headless: bool,
}

#[derive(Debug)]
pub struct TestArgs {
    pub paths: Vec<PathBuf>,
    pub env: Vec<String>,
    pub env_file: Option<PathBuf>,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub format: String,
    pub output: Option<PathBuf>,
    pub test_suite_name: String,
    pub config: Option<PathBuf>,
    pub debug_output: Option<PathBuf>,
    pub find_timeout: Option<u64>,
    pub retries: u32,
    pub shard_split: Option<usize>,
    pub shard_all: Option<usize>,
    pub app_file: Vec<PathBuf>,
    pub continuous: bool,
    pub video: bool,
    pub keep_animations: bool,
    pub no_ansi: bool,
    pub headless: bool,
    pub flatten_debug_output: bool,
    pub analyze: bool,
}

/// What the command line asks for.
#[derive(Debug)]
pub enum Parsed {
    Run(Cli),
    /// Print this and exit 0.
    Help(String),
    Version(String),
    /// Print this and exit 2.
    Error(String),
}

pub const VERSION: &str = concat!("virtuoso ", env!("CARGO_PKG_VERSION"), " (agent ", env!("VIRTUOSO_AGENT_HASH"), ")");
const ABOUT: &str = "A fast, Maestro-compatible end-to-end test runner for mobile apps";

/// An option: `value` names its value, or it is a flag; an empty `help` hides it.
struct Opt {
    long: &'static str,
    short: Option<char>,
    aliases: &'static [&'static str],
    value: Option<&'static str>,
    help: &'static str,
}

const fn flag(long: &'static str, help: &'static str) -> Opt {
    Opt { long, short: None, aliases: &[], value: None, help }
}

const fn opt(long: &'static str, value: &'static str, help: &'static str) -> Opt {
    Opt { long, short: None, aliases: &[], value: Some(value), help }
}

const GLOBAL: &[Opt] = &[
    Opt { aliases: &["udid"], ..opt("device", "DEVICE", "Device serial(s), comma-separated (default: the first adb device)") },
    flag("auto-start-emulator", "With no device connected, start the first emulator (AVD) and wait for it to boot"),
    opt("start-emulator", "AVD", "With no device connected, start this AVD"),
    Opt { short: Some('p'), ..opt("platform", "PLATFORM", "`android` or `ios` (a booted simulator) [default: android]") },
];

const TEST: &[Opt] = &[
    Opt { short: Some('e'), ..opt("env", "KEY=VALUE", "Visible to flows as ${KEY}") },
    opt("env-file", "FILE", "A file of KEY=VALUE lines; -e wins over it"),
    opt("include-tags", "TAGS", "Run only flows with one of these tags (comma-separated)"),
    opt("exclude-tags", "TAGS", "Leave out flows with one of these tags (comma-separated)"),
    opt("format", "FORMAT", "junit, html, json, allure (a results folder) or noop [default: noop]"),
    opt("output", "PATH", "Where the report goes"),
    opt("test-suite-name", "NAME", "The suite's name in reports [default: Test Suite]"),
    opt("config", "FILE", "The workspace config (default: config.yaml in the folder)"),
    Opt { aliases: &["test-output-dir"], ..opt("debug-output", "DIR", "Where screenshots, recordings and failure details go") },
    opt("find-timeout", "MS", "How long a command waits for its element, in ms (default 17000, as Maestro)"),
    opt("retries", "N", "Run a failing flow up to N more times; it passes if any attempt passes [default: 0]"),
    Opt { aliases: &["shards"], ..opt("shard-split", "N", "Split the flows across N devices") },
    opt("shard-all", "N", "Run every flow on each of N devices"),
    opt("app-file", "APK", "Install (or update) this APK on each device before the run (repeatable)"),
    Opt { short: Some('c'), ..flag("continuous", "Keep running: run the flows again whenever a flow or script changes") },
    Opt { aliases: &["record"], ..flag("video", "Record a video of every flow into the debug output folder") },
    flag("keep-animations", "Leave the device's animations on (virtuoso turns them off by default)"),
    Opt { aliases: &["plain"], ..flag("no-ansi", "Plain output, no colours") },
    // Accepted for Maestro compatibility; they change nothing on a local run.
    flag("headless", ""),
    flag("flatten-debug-output", ""),
    flag("analyze", ""),
];

const LINT: &[Opt] = &[flag("headless", "")];
const SKILLS: &[Opt] = &[opt("dir", "DIR", "Folder to write SKILL.md into"), flag("print", "Print the skill instead of installing it")];
const ALL: &[Opt] = &[flag("all", "Every node, not only those a selector would match")];
const LATENCY: &[Opt] = &[opt("count", "N", "Round trips per operation [default: 20]")];
const APP: &[Opt] = &[opt("app", "APP", "The app's id")];

/// What a command takes besides options.
#[derive(Clone, Copy, PartialEq)]
enum Pos {
    None,
    Optional(&'static str),
    Required(&'static str),
    Many(&'static str),
}

struct Spec {
    name: &'static str,
    about: &'static str,
    opts: &'static [Opt],
    pos: Pos,
    hidden: bool,
}

const fn spec(name: &'static str, about: &'static str, opts: &'static [Opt], pos: Pos) -> Spec {
    Spec { name, about, opts, pos, hidden: false }
}

const COMMANDS: &[Spec] = &[
    spec("test", "Run flows (files or folders)", TEST, Pos::Many("PATHS")),
    spec("lint", "Check flows (and the flows they run) without a device", LINT, Pos::Many("PATHS")),
    spec("devices", "List the devices virtuoso can use", &[], Pos::None),
    spec("mcp", "Serve the Model Context Protocol on stdio, for AI agents", &[], Pos::None),
    spec("screenshot", "Save a screenshot of the device (default screenshot.png)", &[], Pos::Optional("PATH")),
    spec("skills", "Install the virtuoso skill for AI coding agents (default ~/.claude/skills/virtuoso)", SKILLS, Pos::None),
    spec("doctor", "Check what virtuoso needs: adb, a device, curl for scripts' http (on iOS, the agent, built if it is not yet)", &[], Pos::None),
    spec("session", "Keep a device connected and drive an app a command at a time (`look`, `do`)", &[], Pos::None),
    spec("look", "What is on screen now, in the session", ALL, Pos::None),
    spec("do", "Run Maestro commands in the session, in order: `virtuoso do launchApp 'tapOn: Sign in'`", &[], Pos::Many("COMMANDS")),
    spec("latency", "Measure agent round trips on the device (ping, screen read, screenshot)", LATENCY, Pos::None),
    spec("hierarchy", "Print what is on the device's screen", ALL, Pos::None),
];

const SESSION: &[Spec] = &[
    spec("start", "Connect to the device and keep it connected for `look` and `do`", APP, Pos::None),
    spec("log", "The commands that passed so far", &[], Pos::None),
    spec("save", "Write the commands that passed as a flow", &[], Pos::Required("PATH")),
    spec("clear", "Forget the commands so far", &[], Pos::None),
    spec("stop", "Disconnect and end the session", &[], Pos::None),
    Spec { hidden: true, ..spec("serve", "The session itself (started by `session start`)", APP, Pos::None) },
];

/// Options given, by long name, and the rest in order.
#[derive(Default)]
struct Given {
    /// A few at most: a list, not a map (no hasher's random keys to fetch for `--version`).
    values: Vec<(&'static str, Vec<String>)>,
    flags: Vec<&'static str>,
    positional: Vec<String>,
}

impl Given {
    fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }
    fn values(&self, name: &str) -> Option<&Vec<String>> {
        self.values.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
    fn one(&self, name: &str) -> Option<String> {
        self.values(name).and_then(|v| v.last().cloned())
    }
    fn all(&self, name: &str) -> Vec<String> {
        self.values(name).cloned().unwrap_or_default()
    }
    fn path(&self, name: &str) -> Option<PathBuf> {
        self.one(name).map(PathBuf::from)
    }
    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        match self.one(name) {
            None => Ok(None),
            Some(v) => v.parse().map(Some).map_err(|_| format!("invalid value '{v}' for '--{name}': a whole number is needed")),
        }
    }
}

fn find<'a>(opts: &'a [Opt], long: &str) -> Option<&'a Opt> {
    opts.iter().find(|o| o.long == long || o.aliases.contains(&long))
}

fn find_short(opts: &[Opt], c: char) -> Option<&Opt> {
    opts.iter().find(|o| o.short == Some(c))
}

/// The usage line of `path` (the command words after `virtuoso`).
fn usage(path: &[&Spec]) -> String {
    let mut u = String::from("Usage: virtuoso");
    for s in path {
        let _ = write!(u, " {}", s.name);
    }
    let last = path.last();
    if last.is_none_or(|s| !s.opts.is_empty()) || path.is_empty() {
        u.push_str(" [OPTIONS]");
    }
    match last {
        None => u.push_str(" <COMMAND>"),
        Some(s) if s.name == "session" => u.push_str(" <COMMAND>"),
        Some(s) => match s.pos {
            Pos::None => {}
            Pos::Optional(n) => {
                let _ = write!(u, " [{n}]");
            }
            Pos::Required(n) => {
                let _ = write!(u, " <{n}>");
            }
            Pos::Many(n) => {
                let _ = write!(u, " [{n}]...");
            }
        },
    }
    u
}

fn options_help(h: &mut String, opts: &[&Opt], version: bool) {
    let label = |o: &Opt| {
        let short = o.short.map(|c| format!("-{c}, ")).unwrap_or_else(|| "    ".into());
        let value = o.value.map(|v| format!(" <{v}>")).unwrap_or_default();
        format!("{short}--{}{value}", o.long)
    };
    let width = opts.iter().map(|o| label(o).len()).max().unwrap_or(0).max(13);
    for o in opts {
        let _ = writeln!(h, "  {:width$}  {}", label(o), o.help);
    }
    let _ = writeln!(h, "  {:width$}  Print help", "-h, --help");
    if version {
        let _ = writeln!(h, "  {:width$}  Print version", "-V, --version");
    }
}

fn help_for(path: &[&Spec]) -> String {
    let mut h = String::new();
    let _ = writeln!(h, "{}\n\n{}\n", path.last().map(|s| s.about).unwrap_or(ABOUT), usage(path));
    let subcommands: &[Spec] = match path.last() {
        None => COMMANDS,
        Some(s) if s.name == "session" => SESSION,
        Some(_) => &[],
    };
    if !subcommands.is_empty() {
        h.push_str("Commands:\n");
        for c in subcommands.iter().filter(|c| !c.hidden) {
            let _ = writeln!(h, "  {:12}{}", c.name, c.about);
        }
        let _ = writeln!(h, "  {:12}Print help for a command\n", "help");
    }
    let mut opts: Vec<&Opt> = path.last().map(|s| s.opts.iter().collect()).unwrap_or_default();
    opts.extend(GLOBAL);
    opts.retain(|o| !o.help.is_empty());
    h.push_str("Options:\n");
    options_help(&mut h, &opts, path.is_empty());
    h
}

fn error(path: &[&Spec], msg: &str) -> Parsed {
    Parsed::Error(format!("error: {msg}\n\n{}\n\nFor more information, try '--help'.", usage(path)))
}

/// Parses `args` (with the program name first).
pub fn parse(args: impl IntoIterator<Item = String>) -> Parsed {
    let mut args = args.into_iter().skip(1).peekable();
    let mut path: Vec<&Spec> = Vec::new();
    let mut global = Given::default();
    let mut given = Given::default();
    let mut help = false;
    let mut only_positional = false;
    while let Some(arg) = args.next() {
        let opts = path.last().map(|s| s.opts).unwrap_or(&[]);
        if only_positional || arg == "-" || !arg.starts_with('-') {
            let wants_command = path.is_empty() || path.last().is_some_and(|s| s.name == "session" && path.len() == 1);
            if wants_command && !only_positional {
                let list = if path.is_empty() { COMMANDS } else { SESSION };
                if arg == "help" && path.is_empty() {
                    help = true;
                    continue;
                }
                match list.iter().find(|c| c.name == arg) {
                    Some(c) => path.push(c),
                    None => return error(&path, &format!("unrecognized subcommand '{arg}'")),
                }
                continue;
            }
            given.positional.push(arg);
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }
        if arg == "-h" || arg == "--help" {
            help = true;
            continue;
        }
        if arg == "-V" || arg == "--version" {
            return Parsed::Version(VERSION.to_string());
        }
        // The option, and a value given with it (`--name=value`, `-evalue`).
        let (o, attached) = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            match find(opts, name).map(|o| (o, false)).or_else(|| find(GLOBAL, name).map(|o| (o, true))) {
                Some((o, is_global)) => ((o, is_global), value),
                None => return error(&path, &format!("unexpected argument '--{name}' found")),
            }
        } else {
            let mut chars = arg[1..].chars();
            let c = chars.next().unwrap_or('-');
            let rest: String = chars.collect();
            match find_short(opts, c).map(|o| (o, false)).or_else(|| find_short(GLOBAL, c).map(|o| (o, true))) {
                Some(found) => (found, (!rest.is_empty()).then_some(rest)),
                None => return error(&path, &format!("unexpected argument '{arg}' found")),
            }
        };
        let ((o, is_global), attached) = (o, attached);
        let target = if is_global { &mut global } else { &mut given };
        match (o.value, attached) {
            (None, Some(_)) => return error(&path, &format!("unexpected value for '--{}': it takes none", o.long)),
            (None, None) => target.flags.push(o.long),
            (Some(v), value) => {
                let value = match value.or_else(|| args.next_if(|_| true)) {
                    Some(value) => value,
                    None => return error(&path, &format!("a value is required for '--{} <{v}>' but none was supplied", o.long)),
                };
                let i = match target.values.iter().position(|(n, _)| *n == o.long) {
                    Some(i) => i,
                    None => {
                        target.values.push((o.long, Vec::new()));
                        target.values.len() - 1
                    }
                };
                let values = &mut target.values[i].1;
                if o.long.ends_with("-tags") {
                    values.extend(value.split(',').filter(|t| !t.is_empty()).map(str::to_string));
                } else {
                    values.push(value);
                }
            }
        }
    }
    if help {
        // `help test` is `test --help`.
        if path.is_empty() {
            if let Some(c) = given.positional.first().and_then(|w| COMMANDS.iter().find(|c| c.name == w)) {
                path.push(c);
            }
        }
        return Parsed::Help(help_for(&path));
    }
    match build(&path, global, given) {
        Ok(cli) => Parsed::Run(cli),
        Err(msg) => error(&path, &msg),
    }
}

fn build(path: &[&Spec], global: Given, g: Given) -> Result<Cli, String> {
    let Some(spec) = path.first() else { return Err("a command is required".into()) };
    let leaf = path.last().unwrap();
    match leaf.pos {
        Pos::None if !g.positional.is_empty() => return Err(format!("unexpected argument '{}' found", g.positional[0])),
        Pos::Optional(_) if g.positional.len() > 1 => return Err(format!("unexpected argument '{}' found", g.positional[1])),
        Pos::Required(n) if g.positional.is_empty() => return Err(format!("the argument <{n}> is required")),
        _ => {}
    }
    let paths = || g.positional.iter().map(PathBuf::from).collect::<Vec<_>>();
    let app = || g.one("app").ok_or_else(|| "the option '--app <APP>' is required".to_string());
    let command = match spec.name {
        "test" => Command::Test(Box::new(TestArgs {
            paths: paths(),
            env: g.all("env"),
            env_file: g.path("env-file"),
            include_tags: g.all("include-tags"),
            exclude_tags: g.all("exclude-tags"),
            format: g.one("format").unwrap_or_else(|| "noop".into()),
            output: g.path("output"),
            test_suite_name: g.one("test-suite-name").unwrap_or_else(|| "Test Suite".into()),
            config: g.path("config"),
            debug_output: g.path("debug-output"),
            find_timeout: g.number("find-timeout")?,
            retries: g.number("retries")?.unwrap_or(0),
            shard_split: g.number("shard-split")?,
            shard_all: g.number("shard-all")?,
            app_file: g.all("app-file").into_iter().map(PathBuf::from).collect(),
            continuous: g.flag("continuous"),
            video: g.flag("video"),
            keep_animations: g.flag("keep-animations"),
            no_ansi: g.flag("no-ansi"),
            headless: g.flag("headless"),
            flatten_debug_output: g.flag("flatten-debug-output"),
            analyze: g.flag("analyze"),
        })),
        "lint" => Command::Lint(LintArgs { paths: paths(), headless: g.flag("headless") }),
        "devices" => Command::Devices,
        "mcp" => Command::Mcp,
        "screenshot" => Command::Screenshot { path: g.positional.first().map(PathBuf::from) },
        "skills" => Command::Skills { dir: g.path("dir"), print: g.flag("print") },
        "doctor" => Command::Doctor,
        "look" => Command::Look { all: g.flag("all") },
        "do" => Command::Do { commands: g.positional.clone() },
        "latency" => Command::Latency { count: g.number("count")?.unwrap_or(20) },
        "hierarchy" => Command::Hierarchy { all: g.flag("all") },
        "session" => Command::Session(match path.get(1).map(|s| s.name) {
            Some("start") => SessionCmd::Start { app: app()? },
            Some("log") => SessionCmd::Log,
            Some("save") => SessionCmd::Save { path: PathBuf::from(&g.positional[0]) },
            Some("clear") => SessionCmd::Clear,
            Some("stop") => SessionCmd::Stop,
            Some("serve") => SessionCmd::Serve { app: app()? },
            _ => return Err("a session command is required (start, log, save, clear, stop)".into()),
        }),
        other => unreachable!("command {other} has no builder"),
    };
    Ok(Cli {
        device: global.one("device"),
        auto_start_emulator: global.flag("auto-start-emulator"),
        start_emulator: global.one("start-emulator"),
        platform: global.one("platform").unwrap_or_else(|| "android".into()),
        command,
    })
}

/// The command line of this process; prints help, the version or an error and exits when
/// that is what it asks for.
pub fn from_env() -> Cli {
    match parse(std::env::args()) {
        Parsed::Run(cli) => cli,
        Parsed::Help(h) => {
            print!("{h}");
            std::process::exit(0);
        }
        Parsed::Version(v) => {
            println!("{v}");
            std::process::exit(0);
        }
        Parsed::Error(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}

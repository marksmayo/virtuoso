//! The command line, parsed without clap: what `virtuoso` and `maestro test` users type.

use std::path::PathBuf;
use virtuoso::args::{parse, Command, Parsed, SessionCmd};

fn ok(args: &[&str]) -> virtuoso::args::Cli {
    let mut all = vec!["virtuoso"];
    all.extend_from_slice(args);
    match parse(all.iter().map(|s| s.to_string())) {
        Parsed::Run(cli) => cli,
        other => panic!("{args:?}: {other:?}"),
    }
}

fn fails(args: &[&str]) -> String {
    let mut all = vec!["virtuoso"];
    all.extend_from_slice(args);
    match parse(all.iter().map(|s| s.to_string())) {
        Parsed::Error(e) => e,
        other => panic!("{args:?} should fail: {other:?}"),
    }
}

fn help(args: &[&str]) -> String {
    let mut all = vec!["virtuoso"];
    all.extend_from_slice(args);
    match parse(all.iter().map(|s| s.to_string())) {
        Parsed::Help(h) => h,
        other => panic!("{args:?} should print help: {other:?}"),
    }
}

fn test_args(args: &[&str]) -> virtuoso::args::TestArgs {
    match ok(args).command {
        Command::Test(t) => *t,
        _ => panic!("not test"),
    }
}

#[test]
fn global_options_go_before_or_after_the_command() {
    let a = ok(&["--device", "emulator-5554", "test", "a.yaml"]);
    assert_eq!(a.device.as_deref(), Some("emulator-5554"));
    let b = ok(&["test", "a.yaml", "--device=emulator-5556", "--auto-start-emulator"]);
    assert_eq!(b.device.as_deref(), Some("emulator-5556"));
    assert!(b.auto_start_emulator);
    assert_eq!(ok(&["--udid", "X", "devices"]).device.as_deref(), Some("X"));
    assert_eq!(ok(&["--start-emulator", "Pixel", "devices"]).start_emulator.as_deref(), Some("Pixel"));
    assert_eq!(ok(&["devices"]).platform, "android");
    assert_eq!(ok(&["-p", "ios", "devices"]).platform, "ios");
    assert_eq!(ok(&["devices", "--platform=web"]).platform, "web");
}

#[test]
fn test_takes_maestro_tests_options() {
    let t = test_args(&[
        "test", "flows/", "b.yaml", "-e", "USER=ada", "--env=PASS=x=y", "-eN=1", "--env-file", ".env", "--include-tags", "smoke,fast",
        "--include-tags=slow", "--exclude-tags", "wip", "--format", "junit", "--output", "r.xml", "--test-suite-name", "S", "--config",
        "c.yaml", "--test-output-dir", "out", "--find-timeout", "5000", "--retries", "2", "--shards", "3", "--app-file", "a.apk",
        "--app-file", "b.apk", "-c", "--record", "--keep-animations", "--plain",
    ]);
    assert_eq!(t.paths, [PathBuf::from("flows/"), PathBuf::from("b.yaml")]);
    assert_eq!(t.env, ["USER=ada", "PASS=x=y", "N=1"]);
    assert_eq!(t.env_file, Some(PathBuf::from(".env")));
    assert_eq!(t.include_tags, ["smoke", "fast", "slow"]);
    assert_eq!(t.exclude_tags, ["wip"]);
    assert_eq!(t.format, "junit");
    assert_eq!(t.output, Some(PathBuf::from("r.xml")));
    assert_eq!(t.test_suite_name, "S");
    assert_eq!(t.config, Some(PathBuf::from("c.yaml")));
    assert_eq!(t.debug_output, Some(PathBuf::from("out")));
    assert_eq!(t.find_timeout, Some(5000));
    assert_eq!(t.retries, 2);
    assert_eq!(t.shard_split, Some(3));
    assert_eq!(t.shard_all, None);
    assert_eq!(t.app_file, [PathBuf::from("a.apk"), PathBuf::from("b.apk")]);
    assert!(t.continuous && t.video && t.keep_animations && t.no_ansi);
}

#[test]
fn test_options_have_maestros_defaults_and_its_no_op_flags() {
    let t = test_args(&["test", "a.yaml", "--headless", "--flatten-debug-output", "--analyze", "--shard-all", "2", "--debug-output", "d"]);
    assert_eq!(t.format, "noop");
    assert_eq!(t.test_suite_name, "Test Suite");
    assert_eq!(t.retries, 0);
    assert_eq!(t.shard_all, Some(2));
    assert_eq!(t.debug_output, Some(PathBuf::from("d")));
    assert!(t.env.is_empty() && !t.video && !t.continuous);
    // `--` ends the options: what follows is a path even if it looks like one.
    assert_eq!(test_args(&["test", "--", "-odd.yaml"]).paths, [PathBuf::from("-odd.yaml")]);
}

#[test]
fn every_command_parses() {
    assert!(matches!(ok(&["lint", "a.yaml", "b", "--headless"]).command, Command::Lint(l) if l.paths.len() == 2));
    assert!(matches!(ok(&["devices"]).command, Command::Devices));
    assert!(matches!(ok(&["mcp"]).command, Command::Mcp));
    assert!(matches!(ok(&["doctor"]).command, Command::Doctor));
    assert!(matches!(ok(&["screenshot"]).command, Command::Screenshot { path: None }));
    assert!(matches!(ok(&["screenshot", "s.png"]).command, Command::Screenshot { path: Some(p) } if p.as_os_str() == "s.png"));
    assert!(matches!(ok(&["skills", "--dir", "d", "--print"]).command, Command::Skills { dir: Some(_), print: true }));
    assert!(matches!(ok(&["look", "--all"]).command, Command::Look { all: true }));
    assert!(matches!(ok(&["hierarchy"]).command, Command::Hierarchy { all: false }));
    assert!(matches!(ok(&["latency"]).command, Command::Latency { count: 20 }));
    assert!(matches!(ok(&["latency", "--count", "5"]).command, Command::Latency { count: 5 }));
    match ok(&["do", "launchApp", "tapOn: Sign in"]).command {
        Command::Do { commands } => assert_eq!(commands, ["launchApp", "tapOn: Sign in"]),
        _ => panic!(),
    }
    assert!(matches!(ok(&["session", "start", "--app", "com.a"]).command, Command::Session(SessionCmd::Start { app }) if app == "com.a"));
    assert!(matches!(ok(&["session", "serve", "--app=com.a"]).command, Command::Session(SessionCmd::Serve { .. })));
    assert!(matches!(ok(&["session", "save", "f.yaml"]).command, Command::Session(SessionCmd::Save { .. })));
    for (word, want) in [("log", "Log"), ("clear", "Clear"), ("stop", "Stop")] {
        let got = format!("{:?}", match ok(&["session", word]).command {
            Command::Session(s) => s,
            _ => panic!(),
        });
        assert_eq!(got, want);
    }
}

#[test]
fn mistakes_are_errors_that_say_what_is_wrong() {
    assert!(fails(&["test", "--bogus"]).contains("--bogus"));
    assert!(fails(&["test", "a.yaml", "--format"]).contains("--format"), "a value is missing");
    assert!(fails(&["test", "--retries", "many"]).contains("--retries"));
    assert!(fails(&["test", "--video=yes"]).contains("--video"));
    assert!(fails(&["frobnicate"]).contains("frobnicate"));
    assert!(fails(&[]).contains("Usage"), "no command");
    assert!(fails(&["session", "start"]).contains("--app"), "a required option");
    assert!(fails(&["session", "save"]).contains("PATH"), "a required argument");
    assert!(fails(&["devices", "extra"]).contains("extra"));
}

#[test]
fn help_lists_the_commands_and_each_commands_options() {
    let top = help(&["--help"]);
    for c in ["test", "lint", "devices", "hierarchy", "session", "Run flows", "--device"] {
        assert!(top.contains(c), "top help lacks {c}:\n{top}");
    }
    assert_eq!(help(&["-h"]), top);
    assert_eq!(help(&["help"]), top);
    let t = help(&["test", "--help"]);
    for o in ["--env-file", "--include-tags", "--format", "-e, --env", "--retries", "--continuous"] {
        assert!(t.contains(o), "test help lacks {o}:\n{t}");
    }
    assert!(!t.contains("--analyze") && !t.contains("--flatten-debug-output"), "hidden flags stay hidden");
    assert_eq!(help(&["help", "test"]), t);
    assert!(help(&["session", "--help"]).contains("start"));
}

#[test]
fn version_names_the_build() {
    for v in ["--version", "-V"] {
        let mut all = vec!["virtuoso".to_string(), v.to_string()];
        all.push("test".into());
        match parse(all) {
            Parsed::Version(s) => assert!(s.starts_with(&format!("virtuoso {}", env!("CARGO_PKG_VERSION"))), "{s}"),
            other => panic!("{other:?}"),
        }
    }
}

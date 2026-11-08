//! Measuring tool runs.

use std::time::{Duration, Instant};
use virtuoso_bench::measure::{parse_time_line, run, run_for, Runner};

#[test]
fn gnu_time_output_is_read_from_among_other_lines() {
    let out = "Flow passed\nsome noise\nVIRTUOSO_BENCH_TIME 12.34 45678 0.50 0.25 1\n";
    assert_eq!(parse_time_line(out), Some((12340.0, 45678.0, 750.0, 1)));
    assert_eq!(parse_time_line("no timing here"), None);
}

#[test]
fn a_native_run_reports_wall_time_exit_status_and_output() {
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) { ("cmd", vec!["/c", "echo hello& exit 3"]) } else { ("sh", vec!["-c", "echo hello; exit 3"]) };
    let mut argv = vec![prog.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    let m = run(&Runner::Native, &argv, &[], ".").unwrap();
    assert_eq!(m.exit, 3);
    assert!(m.output.contains("hello"));
    assert!(m.wall_ms > 0.0);
    if cfg!(windows) {
        assert!(m.peak_rss_kb.unwrap() > 100.0, "peak working set measured");
        assert!(m.cpu_ms.is_some());
    }
}

#[cfg(windows)]
#[test]
fn native_cpu_time_includes_the_processes_a_tool_starts() {
    // cmd does nothing itself; the PowerShell it starts spins for ~0.6 s. The script is encoded
    // (UTF-16LE base64 of `$t=[DateTime]::Now; while(([DateTime]::Now-$t).TotalMilliseconds -lt 600){}`)
    // so no quoting between cmd and PowerShell can mangle it.
    let argv: Vec<String> = ["cmd", "/c", "powershell -NoProfile -EncodedCommand JAB0AD0AWwBEAGEAdABlAFQAaQBtAGUAXQA6ADoATgBvAHcAOwAgAHcAaABpAGwAZQAoACgAWwBEAGEAdABlAFQAaQBtAGUAXQA6ADoATgBvAHcALQAkAHQAKQAuAFQAbwB0AGEAbABNAGkAbABsAGkAcwBlAGMAbwBuAGQAcwAgAC0AbAB0ACAANgAwADAAKQB7AH0A"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let m = run(&Runner::Native, &argv, &[], ".").unwrap();
    assert!(m.cpu_ms.unwrap() > 300.0, "cpu {:?} ms, wall {} ms, output: {}", m.cpu_ms, m.wall_ms, m.output);
}

#[cfg(windows)]
#[test]
fn a_native_run_counts_the_processes_a_tool_starts() {
    // A tool that starts others (adb.exe, a shell) pays for each: the count shows when it does.
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let alone = run(&Runner::Native, &argv(&["cmd", "/c", "exit 0"]), &[], ".").unwrap();
    assert_eq!(alone.processes, Some(1));
    let nested = run(&Runner::Native, &argv(&["cmd", "/c", "cmd /c exit 0"]), &[], ".").unwrap();
    assert_eq!(nested.processes, Some(2));
}

#[cfg(unix)]
#[test]
fn a_native_unix_run_reports_peak_memory_and_the_cpu_of_what_it_starts() {
    // sh does nothing itself; the awk it starts adds up numbers for a few hundred ms. As GNU time
    // counts it: the tool and the processes it waits for.
    let argv: Vec<String> = ["sh", "-c", "awk 'BEGIN{for(i=0;i<20000000;i++)s+=i; print s}'"].iter().map(|s| s.to_string()).collect();
    let m = run(&Runner::Native, &argv, &[], ".").unwrap();
    assert_eq!(m.exit, 0, "{}", m.output);
    assert!(m.cpu_ms.unwrap() > 100.0, "cpu {:?} ms, wall {} ms", m.cpu_ms, m.wall_ms);
    assert!(m.peak_rss_kb.unwrap() > 100.0, "peak {:?} KB", m.peak_rss_kb);
}

#[test]
fn a_run_that_hangs_is_stopped_at_its_limit_with_what_it_started() {
    // The shell waits on a child that holds the output pipes: both go at the limit, and the run
    // fails saying why instead of holding up the benchmark.
    let argv: Vec<String> = if cfg!(windows) { vec!["cmd".into(), "/c".into(), "ping -n 30 127.0.0.1".into()] } else { vec!["sh".into(), "-c".into(), "sleep 30; echo done".into()] };
    let start = Instant::now();
    let m = run_for(&Runner::Native, &argv, &[], ".", Duration::from_secs(2)).unwrap();
    assert!(start.elapsed() < Duration::from_secs(15), "stopped after {:?}", start.elapsed());
    assert_ne!(m.exit, 0);
    assert!(m.output.contains("timed out after 2 s"), "{}", m.output);
}

#[test]
fn a_wsl_run_that_hangs_is_stopped_at_its_limit() {
    // Only where a WSL distribution is named (VIRTUOSO_BENCH_WSL).
    let Ok(distro) = std::env::var("VIRTUOSO_BENCH_WSL") else { return };
    let argv: Vec<String> = vec!["sh".into(), "-c".into(), "sleep 30; echo done".into()];
    let start = Instant::now();
    let m = run_for(&Runner::Wsl { distro }, &argv, &[], "/tmp", Duration::from_secs(2)).unwrap();
    assert!(start.elapsed() < Duration::from_secs(20), "stopped after {:?}", start.elapsed());
    assert_ne!(m.exit, 0);
    assert!(m.output.contains("timed out after 2 s"), "{}", m.output);
}

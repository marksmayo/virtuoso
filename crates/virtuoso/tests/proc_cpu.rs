//! CPU time from /proc, read from a stand-in /proc: a process's own, and its whole tree's (the
//! run's: virtuoso and the browser it started, as the benchmark counts it).

use std::path::Path;
use virtuoso::proc_cpu::{sample_at, Sample};

/// `<root>/<pid>/stat` as Linux writes it: pid, (comm), state, ppid, ... utime (field 14),
/// stime, cutime, cstime, in clock ticks.
fn stat(root: &Path, pid: u32, comm: &str, ppid: u32, times: [u64; 4]) {
    let dir = root.join(pid.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let [u, s, cu, cs] = times;
    std::fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} {pid} {pid} 0 -1 4194560 100 0 0 0 {u} {s} {cu} {cs} 20 0 1 0 100 1000000 200 18446744073709551615\n")).unwrap();
}

fn proc() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    stat(root, 10, "virtuoso", 1, [50, 10, 0, 0]);
    // The browser: what it reaped of its own children is in its cutime and cstime.
    stat(root, 11, "chrome", 10, [200, 40, 30, 5]);
    // A name with spaces and a parenthesis in it.
    stat(root, 12, "chrome (rend) x", 11, [100, 20, 0, 0]);
    stat(root, 13, "chrome", 12, [7, 3, 0, 0]);
    // Not under the run.
    stat(root, 20, "sshd", 1, [9000, 900, 0, 0]);
    // What else /proc has.
    std::fs::create_dir_all(root.join("self")).unwrap();
    std::fs::create_dir_all(root.join("net")).unwrap();
    std::fs::create_dir_all(root.join("30")).unwrap();
    d
}

#[test]
fn a_process_trees_cpu_is_its_own_and_every_descendants_in_ms() {
    let d = proc();
    // 100 ticks a second: 10 ms a tick.
    let s = sample_at(d.path(), 10, 100).expect("a sample");
    assert_eq!(s, Sample { own_ms: (50 + 10) * 10, tree_ms: (50 + 10 + 200 + 40 + 30 + 5 + 100 + 20 + 7 + 3) * 10 });
    // Another clock rate.
    assert_eq!(sample_at(d.path(), 12, 1000).unwrap(), Sample { own_ms: 120, tree_ms: 130 });
}

#[test]
fn a_process_that_is_gone_has_no_sample() {
    let d = proc();
    assert_eq!(sample_at(d.path(), 99, 100), None);
    // A stat that cannot be read as one.
    std::fs::write(d.path().join("13").join("stat"), "13 (chrome) S").unwrap();
    assert_eq!(sample_at(d.path(), 13, 100), None);
    assert_eq!(sample_at(&d.path().join("missing"), 10, 100), None);
}

#[test]
fn this_process_is_sampled_where_there_is_a_proc() {
    let s = virtuoso::proc_cpu::sample();
    if cfg!(target_os = "linux") {
        let s = s.expect("Linux has /proc");
        assert!(s.tree_ms >= s.own_ms, "{s:?}");
    } else {
        assert_eq!(s, None);
    }
}

//! The mocked location the device holds, for the device tests.

use virtuoso::android::Adb;

/// The gps fix the system holds, and how many seconds old it is.
pub fn gps_fix(adb: &Adb) -> (String, f64) {
    let dump = adb.shell("dumpsys location").unwrap();
    let line = dump.lines().find(|l| l.contains("last mock location=Location[gps")).expect("a mocked gps fix");
    let fix = line.split("Location[gps ").nth(1).unwrap().split(' ').next().unwrap().to_string();
    // et=+1h2m3s456ms: the fix's time since boot.
    let et = line.split("et=+").nth(1).unwrap().split(' ').next().unwrap();
    let (mut at, mut n) = (0.0, String::new());
    let mut units = et.chars().peekable();
    while let Some(c) = units.next() {
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        let v: f64 = std::mem::take(&mut n).parse().unwrap();
        at += match c {
            'h' => v * 3600.0,
            'm' if units.peek() == Some(&'s') => {
                units.next();
                v / 1000.0
            }
            'm' => v * 60.0,
            's' => v,
            'd' => v * 86400.0,
            _ => 0.0,
        };
    }
    let up: f64 = adb.shell("cat /proc/uptime").unwrap().split_whitespace().next().unwrap().parse().unwrap();
    (fix, up - at)
}

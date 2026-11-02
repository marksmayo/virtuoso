//! The latency benchmark's numbers.

use virtuoso::latency::Summary;
use std::time::Duration;

#[test]
fn a_summary_gives_median_95th_percentile_and_worst() {
    let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
    let s = Summary::of(&samples).unwrap();
    assert_eq!(s.p50, Duration::from_millis(50));
    assert_eq!(s.p95, Duration::from_millis(95));
    assert_eq!(s.max, Duration::from_millis(100));
    assert!(Summary::of(&[]).is_none());
}

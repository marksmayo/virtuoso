//! Statistics and medal ranking.

use virtuoso_bench::medals::{medal_table, rank, Better, Medal, MetricResult};
use virtuoso_bench::stats::{cv, median};

#[test]
fn median_of_odd_and_even_samples() {
    assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
    assert_eq!(median(&[]), None);
}

#[test]
fn coefficient_of_variation_is_stddev_over_mean() {
    assert_eq!(cv(&[5.0, 5.0, 5.0]), Some(0.0));
    let c = cv(&[9.0, 10.0, 11.0]).unwrap();
    assert!((c - 0.1).abs() < 1e-9, "{c}");
    assert_eq!(cv(&[1.0]), None);
}

fn m(values: &[(&str, Option<f64>)], better: Better) -> MetricResult {
    MetricResult {
        key: "t".into(),
        label: "T".into(),
        group: "g".into(),
        unit: "s".into(),
        better,
        values: values.iter().map(|(t, v)| (t.to_string(), *v)).collect(),
    }
}

#[test]
fn lowest_wins_gold_when_lower_is_better() {
    let r = rank(&m(&[("a", Some(3.0)), ("b", Some(1.0)), ("c", Some(2.0))], Better::Lower));
    assert_eq!(r["b"], Some(Medal::Gold));
    assert_eq!(r["c"], Some(Medal::Silver));
    assert_eq!(r["a"], Some(Medal::Bronze));
}

#[test]
fn highest_wins_gold_when_higher_is_better() {
    let r = rank(&m(&[("a", Some(0.9)), ("b", Some(1.0))], Better::Higher));
    assert_eq!(r["b"], Some(Medal::Gold));
    assert_eq!(r["a"], Some(Medal::Silver));
}

#[test]
fn ties_share_a_medal_and_the_next_rank_is_skipped() {
    let r = rank(&m(&[("a", Some(1.0)), ("b", Some(1.0)), ("c", Some(2.0))], Better::Lower));
    assert_eq!(r["a"], Some(Medal::Gold));
    assert_eq!(r["b"], Some(Medal::Gold));
    assert_eq!(r["c"], Some(Medal::Bronze), "two golds, then bronze, as in the Olympics");
}

#[test]
fn values_within_two_percent_tie() {
    let r = rank(&m(&[("a", Some(10.0)), ("b", Some(10.1)), ("c", Some(20.0))], Better::Lower));
    assert_eq!(r["a"], Some(Medal::Gold));
    assert_eq!(r["b"], Some(Medal::Gold));
}

#[test]
fn a_tool_without_a_result_gets_no_medal() {
    let r = rank(&m(&[("a", None), ("b", Some(5.0)), ("c", Some(6.0))], Better::Lower));
    assert_eq!(r["a"], None);
    assert_eq!(r["b"], Some(Medal::Gold));
    assert_eq!(r["c"], Some(Medal::Silver));
}

#[test]
fn the_medal_table_counts_and_orders_like_the_olympics() {
    let metrics = vec![
        m(&[("a", Some(1.0)), ("b", Some(2.0)), ("c", Some(3.0))], Better::Lower),
        m(&[("a", Some(2.0)), ("b", Some(1.0)), ("c", Some(3.0))], Better::Lower),
        m(&[("a", Some(1.0)), ("b", Some(3.0)), ("c", Some(2.0))], Better::Lower),
    ];
    let t = medal_table(&metrics, &["a".into(), "b".into(), "c".into()]);
    assert_eq!(t[0].tool, "a");
    assert_eq!((t[0].gold, t[0].silver, t[0].bronze), (2, 1, 0));
    assert_eq!(t[1].tool, "b");
    assert_eq!((t[1].gold, t[1].silver, t[1].bronze), (1, 1, 1));
    assert_eq!(t[2].tool, "c");
    assert_eq!((t[2].gold, t[2].silver, t[2].bronze), (0, 1, 2));
}

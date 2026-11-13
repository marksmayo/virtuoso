//! The small stable sort used for short lists (flows, paths, secrets).

use virtuoso::sort::sort_small_by_key;

#[test]
fn it_sorts_by_key_and_keeps_equal_items_in_order() {
    let mut v = vec![(3, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
    sort_small_by_key(&mut v, |x| x.0);
    assert_eq!(v, [(1, 'b'), (1, 'e'), (2, 'd'), (3, 'a'), (3, 'c')]);
    let mut empty: Vec<u8> = vec![];
    sort_small_by_key(&mut empty, |x| *x);
    let mut words = vec!["pear".to_string(), "fig".into(), "apple".into()];
    sort_small_by_key(&mut words, |w| std::cmp::Reverse(w.len()));
    assert_eq!(words, ["apple", "pear", "fig"]);
}

#[test]
fn each_key_is_worked_out_once() {
    // Keys can cost (a flow's relative path is two allocations): once per item, not twice per
    // comparison.
    let calls = std::cell::Cell::new(0);
    let mut v: Vec<u32> = (0..50).rev().collect();
    sort_small_by_key(&mut v, |x| {
        calls.set(calls.get() + 1);
        *x
    });
    assert_eq!(v, (0..50).collect::<Vec<_>>());
    assert_eq!(calls.get(), 50);
}

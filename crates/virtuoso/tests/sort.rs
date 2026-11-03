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

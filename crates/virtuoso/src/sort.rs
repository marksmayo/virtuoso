//! A stable insertion sort for the short lists virtuoso orders (flows, paths, secrets). The
//! standard library's stable sort is ~5-13 KB of the binary for each type it sorts.

/// Sorts `v` by `key`, keeping equal items in their order. Quadratic: for short lists only.
/// Each item's key is worked out once and moved along with it.
pub fn sort_small_by_key<T, K: Ord>(v: &mut [T], key: impl Fn(&T) -> K) {
    let mut keys: Vec<K> = v.iter().map(key).collect();
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && keys[j - 1] > keys[j] {
            keys.swap(j - 1, j);
            v.swap(j - 1, j);
            j -= 1;
        }
    }
}

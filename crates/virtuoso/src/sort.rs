//! A stable insertion sort for the short lists virtuoso orders (flows, paths, secrets). The
//! standard library's stable sort is ~5-13 KB of the binary for each type it sorts.

/// Sorts `v` by `key`, keeping equal items in their order. Quadratic: for short lists only.
pub fn sort_small_by_key<T, K: Ord>(v: &mut [T], key: impl Fn(&T) -> K) {
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && key(&v[j - 1]) > key(&v[j]) {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
}

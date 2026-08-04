#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

/// A hash map using the fast, deterministic Fx hasher.
pub type HashMap<K, V> = FxHashMap<K, V>;
/// A hash set using the fast, deterministic Fx hasher.
pub type HashSet<T> = FxHashSet<T>;
/// An insertion-ordered map using the fast, deterministic Fx hasher.
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, rustc_hash::FxBuildHasher>;
/// An insertion-ordered set using the fast, deterministic Fx hasher.
pub type IndexSet<T> = indexmap::IndexSet<T, rustc_hash::FxBuildHasher>;

pub use indexmap::Equivalent;
pub use rustc_hash::FxHasher;
pub use rustc_hash::{FxHashMap, FxHashSet};
pub use std::collections::*;

#[cfg(test)]
mod tests {
    use super::{FxHashMap, HashMap, IndexMap};

    #[test]
    fn aliases_construct_and_preserve_expected_collection_behavior() {
        let mut hash = HashMap::default();
        hash.insert("alpha", 1);
        let _: &FxHashMap<&str, i32> = &hash;
        assert_eq!(hash.get("alpha"), Some(&1));

        let mut ordered = IndexMap::default();
        ordered.insert("second", 2);
        ordered.insert("first", 1);
        assert_eq!(
            ordered.keys().copied().collect::<Vec<_>>(),
            ["second", "first"]
        );
    }
}

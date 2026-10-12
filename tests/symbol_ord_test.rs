//! `Symbol: Ord` - the order is first-seen order, so it can key ordered
//! containers.

use std::collections::{BTreeMap, BTreeSet};
use winnow_grammar::InternerContext;

#[test]
fn order_follows_first_seen_not_alphabet() {
    let cx = InternerContext::new();
    let z = cx.intern_string("zebra");
    let a = cx.intern_string("apple");
    assert!(z < a);
    assert_eq!(z, cx.intern_string("zebra"));
    assert_eq!(z.cmp(&z), std::cmp::Ordering::Equal);
}

#[test]
fn usable_as_btree_key() {
    let cx = InternerContext::new();
    let (b, a) = (cx.intern_string("b"), cx.intern_string("a"));
    let map: BTreeMap<_, _> = [(a, 1), (b, 2)].into_iter().collect();
    assert_eq!(map.keys().copied().collect::<Vec<_>>(), vec![b, a]);
    let set: BTreeSet<_> = [a, b, a].into_iter().collect();
    assert_eq!(set.len(), 2);
}

//! A lookup cache in front of the interner.
//!
//! `InternerContext::intern_string` costs ~21 ns on a hit: ~14 ns to hash the
//! text and probe the map, ~6 ns for the dashmap shard lock, ~1.5 ns of
//! lasso's own bookkeeping (`benches/where.rs`). A parse spends that on every
//! identifier and every interned field, and it spends it on the *same few
//! words over and over* - which is what a cache is for, and this one skips all
//! three parts, the lock included.
//!
//! It is a cache and nothing else: a miss falls through to the interner, which
//! remains the only authority on what a `Symbol` is. Nothing here can make a
//! symbol wrong; the worst it can do is not help.

use crate::{InternerContext, Symbol};
use std::sync::{Arc, Mutex};

/// One slot. 24 bytes, so the table below is 12 KiB.
#[derive(Clone, Copy)]
struct Slot {
    /// The first eight bytes of the text, zero-padded, little-endian.
    head: u64,
    /// The last eight bytes of a text longer than eight, zero for a shorter
    /// one. For a text of nine to sixteen bytes the two words overlap and
    /// between them hold every byte.
    tail: u64,
    /// The text's length in bytes. With `head` and `tail` it *is* the key for
    /// anything sixteen bytes or shorter, which is why those hits need no
    /// comparison.
    len: u32,
    /// The symbol's index plus one; zero means the slot is empty.
    sym: u32,
}

/// A table a context no longer needs, kept by the interner its symbols
/// belong to so that the next context on that interner starts with it.
struct Table {
    bits: u32,
    slots: Vec<Slot>,
}

/// The tables an interner keeps for the contexts that use it - see
/// [`InternCache::fill`]. Shared by every clone of one
/// [`InternerContext`], and only by those: a table is handed only to a
/// context interning into the interner that filled it, so every symbol in it
/// is still that interner's.
#[derive(Default)]
pub(crate) struct Stash(Mutex<Vec<Table>>);

impl Stash {
    /// How many tables an interner keeps. One is the common case - one
    /// context after another - and a `par_fold` returns one per piece at
    /// once; keeping a few covers the next run of pieces without letting a
    /// wide one pin megabytes.
    const KEEP: usize = 4;

    fn take(&self, bits: u32) -> Option<Vec<Slot>> {
        let mut tables = self.0.lock().ok()?;
        let at = tables.iter().rposition(|t| t.bits == bits)?;
        Some(tables.swap_remove(at).slots)
    }

    fn put(&self, bits: u32, slots: Vec<Slot>) {
        if let Ok(mut tables) = self.0.lock() {
            if tables.len() < Self::KEEP {
                tables.push(Table { bits, slots });
            }
        }
    }
}

impl std::fmt::Debug for Stash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stash")
    }
}

/// Direct-mapped, fixed size, one slot per bucket: a miss or a mismatch
/// overwrites. There is no collision handling because there is nothing to
/// handle - a displaced entry is simply interned again.
///
/// The table is what makes a [`ParseContext`](crate::ParseContext) expensive
/// to clone - ~98 ns against an `Arc` bump, measured. A parse builds one
/// context and a `par_fold` one per piece, so that is paid once against the
/// 1.4 µs a fresh context costs anyway; a caller that clones one per record
/// would notice.
///
/// Public only because it is a field of [`ParseContext`](crate::ParseContext),
/// which callers build with a struct literal. It has no API and no
/// guarantees; do not name it.
pub struct InternCache {
    /// Empty until the first `intern`, so that a grammar which never interns
    /// pays nothing for it - the same reason the interner behind it is built
    /// on first use.
    ///
    /// The allocation is in an outlined `#[cold]` function on purpose. Written
    /// inline it cost ~6% of the *hit* path, which a parse pays thousands of
    /// times: a `vec![…]` in the hot function is enough to keep it from being
    /// inlined. Out of line, the check is a length load that predicts
    /// perfectly.
    slots: Vec<Slot>,
    /// Which interner the slots belong to. A context whose interner is
    /// replaced between parses gets an empty cache rather than another
    /// interner's numbers.
    interner: usize,
    /// The stash of the interner the slots belong to, where they go when this
    /// cache is dropped or rebound. `None` until the first `intern`.
    home: Option<Arc<Stash>>,
    /// `1 << bits` slots. Settable per context, because how many distinct
    /// strings a parse will see is the caller's knowledge - see
    /// [`ParseContext::expect_distinct_keys`](crate::ParseContext::expect_distinct_keys).
    bits: u32,
}

impl InternCache {
    /// 512 slots, 12 KiB. L1d is 32-48 KiB *in total* and the parse also wants
    /// the input, the stack and its output in there, so the table is sized to
    /// leave room rather than to fill the cache. Right for a compiler's
    /// identifier stream, where a few words are hot; a caller whose parse sees
    /// hundreds of distinct keys says so instead.
    const DEFAULT_BITS: u32 = 9;
    /// 64 slots (1 KiB) to 65 536 (1 MiB). The floor is where a table stops
    /// being worth its own cache line; the ceiling is where it stops fitting
    /// in any cache and a hit costs what a miss would have.
    const MIN_BITS: u32 = 6;
    const MAX_BITS: u32 = 16;

    fn empty_slot() -> Slot {
        Slot {
            head: 0,
            tail: 0,
            len: 0,
            sym: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            interner: 0,
            home: None,
            bits: Self::DEFAULT_BITS,
        }
    }

    /// Size the table for a parse that will see about `keys` distinct strings.
    ///
    /// The table gets the next power of two at or above `2 * keys`: it is
    /// direct-mapped with no collision handling, so leaving half of it empty is
    /// what keeps most keys in a slot of their own. Clamped to
    /// [`MIN_BITS`](Self::MIN_BITS)..=[`MAX_BITS`](Self::MAX_BITS).
    ///
    /// The table is emptied, not rehashed: it is a cache, a lost entry costs
    /// one interner call, and rehashing would cost more than that for every
    /// entry that is never looked up again.
    pub(crate) fn size_for(&mut self, keys: usize) {
        // `checked_`, because a caller who says `usize::MAX` means "as big as
        // it goes" and the clamp below is what answers that - not a panic.
        let wanted = keys
            .saturating_mul(2)
            .max(1)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX);
        let bits = (usize::BITS - 1 - wanted.leading_zeros().min(usize::BITS - 1))
            .clamp(Self::MIN_BITS, Self::MAX_BITS);
        if bits != self.bits {
            self.give_back();
            self.bits = bits;
        }
    }

    /// The key a slot is found and rejected by: the first eight bytes and,
    /// for a longer text, the last eight.
    ///
    /// Up to sixteen bytes the two words and the length *are* the text, so a
    /// hit needs no comparison - which matters because the comparison is
    /// `resolve`, and `ThreadedRodeo::resolve` is not an index but a lookup in
    /// a `DashMap` keyed by the symbol: a hash and a shard lock, on every hit
    /// of every name longer than the key. 1BRC's station names are mostly
    /// between nine and sixteen bytes. Longer texts share a key now and then
    /// and are verified as before.
    ///
    /// Eight bytes or fewer are folded byte by byte rather than copied,
    /// because `copy_from_slice` with a length the compiler does not know
    /// becomes a call to `memcpy`, and that call costs more than the whole
    /// rest of a lookup (measured: 13.2 ns a lookup against 8.1). Longer
    /// texts are read as two unaligned words, overlapping below sixteen.
    #[inline]
    fn key(text: &str) -> (u64, u64) {
        let b = text.as_bytes();
        if b.len() > 8 {
            let head = u64::from_le_bytes(b[..8].try_into().expect("eight bytes"));
            let tail = u64::from_le_bytes(b[b.len() - 8..].try_into().expect("eight bytes"));
            return (head, tail);
        }
        let mut head = 0u64;
        for (i, &c) in b.iter().enumerate() {
            head |= (c as u64) << (i * 8);
        }
        (head, 0)
    }

    #[inline]
    fn index(&self, head: u64, tail: u64) -> usize {
        // Both words, so that `identifier_0001` and `identifier_0002` - one
        // head, different tails - land in different buckets.
        let tag = head ^ tail.rotate_left(31);
        // A multiply folds the low bits upward; the top `bits` are then the
        // bucket. Fixed constant: the cache is not adversary-facing - a
        // collision costs one interner call, not a quadratic blow-up.
        (tag.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - self.bits)) as usize
    }

    /// Empties the cache if it belongs to another interner. Called where a
    /// parse begins, which is the only moment the interner can have changed.
    pub(crate) fn rebind(&mut self, interner: &InternerContext) {
        let id = interner.id();
        if self.interner != id {
            self.give_back();
            self.interner = id;
        }
    }

    /// Returns the table to the interner it was filled from, warm, for the
    /// next context on that interner.
    fn give_back(&mut self) {
        let slots = std::mem::take(&mut self.slots);
        if let (false, Some(home)) = (slots.is_empty(), &self.home) {
            home.put(self.bits, slots);
        }
    }

    /// The symbol for `text`, from the cache when it is there and from the
    /// interner otherwise.
    /// Out of line and marked cold: it runs once per context that interns.
    ///
    /// The table comes from the interner when a context before this one left
    /// one there, and is allocated only when none did. A compiler that builds
    /// a context per short parse - Nikaia builds ~870 per compile - otherwise
    /// pays for a fresh table every time: the allocation, writing every slot
    /// empty, and then a miss on every name the previous context had already
    /// cached. Handed on, the table is warm.
    #[cold]
    #[inline(never)]
    fn fill(&mut self, interner: &InternerContext) {
        let home = Arc::clone(interner.stash());
        self.slots = home
            .take(self.bits)
            .unwrap_or_else(|| vec![Self::empty_slot(); 1 << self.bits]);
        self.interner = interner.id();
        self.home = Some(home);
    }

    #[inline]
    pub(crate) fn intern(&mut self, interner: &InternerContext, text: &str) -> Symbol {
        if self.slots.is_empty() {
            self.fill(interner);
        }
        let (head, tail) = Self::key(text);
        let len = text.len() as u32;
        let at = self.index(head, tail);
        let slot = &mut self.slots[at];

        if slot.sym != 0 && slot.head == head && slot.tail == tail && slot.len == len {
            let sym = Symbol::from_index(slot.sym - 1);
            // Up to sixteen bytes the key *is* the text, so a hit needs no
            // comparison. Longer texts can share one, and are verified.
            if len <= 16 || interner.resolve(sym) == text {
                return sym;
            }
        }

        let sym = interner.intern_string(text);
        *slot = Slot {
            head,
            tail,
            len,
            sym: sym.index() + 1,
        };
        sym
    }
}

impl Default for InternCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for InternCache {
    /// A clone is *empty*, but the same size. `ParseContext` is cloned once
    /// per piece of a `par_fold`, and copying twelve kilobytes of cache into a
    /// piece that has not parsed anything yet would cost more than the misses
    /// it saves - a cache has no contents worth preserving. Its size is
    /// another matter: that is what the caller said with
    /// [`expect_distinct_keys`](crate::ParseContext::expect_distinct_keys),
    /// and the pieces of a `par_fold` are where an aggregation over many keys
    /// runs. A clone that forgot it would put every piece back at 512 slots.
    fn clone(&self) -> Self {
        let mut clone = Self::new();
        clone.bits = self.bits;
        clone
    }
}

impl Drop for InternCache {
    fn drop(&mut self) {
        self.give_back();
    }
}

impl std::fmt::Debug for InternCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InternCache")
    }
}

#[cfg(test)]
mod tests {
    use super::InternCache;
    use crate::InternerContext;

    /// The cache is a cache: whatever its size, the symbol it hands back is
    /// the interner's, and the same text twice is the same symbol.
    #[test]
    fn every_size_agrees_with_the_interner() {
        let words: Vec<String> = (0..500).map(|i| format!("station_{i}")).collect();

        for keys in [0, 1, 8, 413, 100_000] {
            let interner = InternerContext::new();
            let mut cache = InternCache::new();
            cache.size_for(keys);
            cache.rebind(&interner);

            for w in &words {
                let through_cache = cache.intern(&interner, w);
                assert_eq!(through_cache, interner.intern_string(w), "{w} at {keys}");
                // And again, which is the path the cache exists for.
                assert_eq!(cache.intern(&interner, w), through_cache);
            }
        }
    }

    /// `keys` asks for twice as many slots, rounded up, and the clamp holds at
    /// both ends.
    #[test]
    fn the_table_is_sized_from_the_key_count() {
        let cases = [
            (0, InternCache::MIN_BITS),
            (413, 10), // 826 -> 1024
            (512, 10), // 1024
            (513, 11), // 1026 -> 2048
            (usize::MAX, InternCache::MAX_BITS),
        ];
        for (keys, bits) in cases {
            let mut cache = InternCache::new();
            cache.size_for(keys);
            assert_eq!(cache.bits, bits, "{keys} keys");
        }
    }

    /// A clone starts empty and keeps the size it was given: `par_fold`
    /// clones the context once per piece, and the pieces are where the keys
    /// are.
    #[test]
    fn a_clone_is_empty_and_keeps_its_size() {
        let interner = InternerContext::new();
        let mut cache = InternCache::new();
        cache.size_for(5_000);
        cache.rebind(&interner);
        cache.intern(&interner, "Hamburg");

        let clone = cache.clone();
        assert!(clone.slots.is_empty(), "a clone carries no entries");
        assert_eq!(clone.bits, cache.bits, "a clone keeps the size");

        let mut ctx = crate::ParseContext::<()>::default();
        ctx.expect_distinct_keys(5_000);
        assert_eq!(ctx.clone().intern_cache.bits, 14);
    }

    /// Up to sixteen bytes the key is the text; beyond, two texts can share
    /// a key - same first eight bytes, same last eight, same length - and the
    /// one that finds the other's slot must not take its symbol.
    #[test]
    fn a_shared_key_is_verified() {
        let interner = InternerContext::new();
        let mut cache = InternCache::new();
        cache.rebind(&interner);
        let a = "stationX_station";
        let b = "stationY_station";
        assert_ne!(cache.intern(&interner, a), cache.intern(&interner, b));
        let long_a = "station_X_station";
        let long_b = "station_Y_station";
        let sa = cache.intern(&interner, long_a);
        let sb = cache.intern(&interner, long_b);
        assert_ne!(sa, sb);
        assert_eq!(cache.intern(&interner, long_a), sa);
        assert_eq!(cache.intern(&interner, long_b), sb);
    }

    /// A cache dropped hands its table to its interner, and the next cache
    /// on that interner starts with it - warm, so the first lookup is a hit.
    #[test]
    fn a_table_is_handed_on_to_the_next_cache_on_the_interner() {
        let interner = InternerContext::new();
        let sym = {
            let mut first = InternCache::new();
            first.rebind(&interner);
            first.intern(&interner, "Hamburg")
        };
        let mut next = InternCache::new();
        next.rebind(&interner);
        next.fill(&interner);
        let at = next.index(InternCache::key("Hamburg").0, 0);
        assert_eq!(
            next.slots[at].sym,
            sym.index() + 1,
            "the table came back warm"
        );
        assert_eq!(next.intern(&interner, "Hamburg"), sym);
    }

    /// Never across interners: another interner's numbers would be wrong
    /// symbols, not slow ones.
    #[test]
    fn a_table_never_reaches_another_interner() {
        let a = InternerContext::new();
        let b = InternerContext::new();
        b.intern_string("padding, so that the numbers differ");
        let in_a = {
            let mut cache = InternCache::new();
            cache.rebind(&a);
            cache.intern(&a, "Hamburg")
        };
        let mut cache = InternCache::new();
        cache.rebind(&b);
        let in_b = cache.intern(&b, "Hamburg");
        assert_eq!(in_b, b.intern_string("Hamburg"));
        assert_ne!(in_a, in_b);

        // Rebinding hands the table back to the interner it came from.
        cache.rebind(&a);
        assert_eq!(cache.intern(&a, "Hamburg"), in_a);
    }

    /// Resizing empties the table, and the emptied table refills itself.
    #[test]
    fn resizing_empties_and_the_cache_refills() {
        let interner = InternerContext::new();
        let mut cache = InternCache::new();
        cache.rebind(&interner);
        let before = cache.intern(&interner, "Hamburg");
        assert!(!cache.slots.is_empty());

        cache.size_for(413);
        assert!(cache.slots.is_empty(), "a resize empties the table");
        assert_eq!(cache.intern(&interner, "Hamburg"), before);
        assert_eq!(cache.slots.len(), 1 << 10);
    }
}

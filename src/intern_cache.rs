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

/// One slot. 16 bytes, so the table below is 8 KiB.
#[derive(Clone, Copy)]
struct Slot {
    /// The first eight bytes of the text, zero-padded, little-endian.
    tag: u64,
    /// The text's length in bytes. With `tag` it *is* the key for anything
    /// eight bytes or shorter, which is why those hits need no comparison.
    len: u32,
    /// The symbol's index plus one; zero means the slot is empty.
    sym: u32,
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
    /// `1 << bits` slots. Settable per context, because how many distinct
    /// strings a parse will see is the caller's knowledge - see
    /// [`ParseContext::expect_distinct_keys`](crate::ParseContext::expect_distinct_keys).
    bits: u32,
}

impl InternCache {
    /// 512 slots, 8 KiB. L1d is 32-48 KiB *in total* and the parse also wants
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
            tag: 0,
            len: 0,
            sym: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            interner: 0,
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
            self.bits = bits;
            self.slots = Vec::new();
        }
    }

    /// The first eight bytes as a `u64`. Reading eight bytes unconditionally
    /// would run past the end of the input - a `&str` from a caller carries no
    /// padding - so a short text is copied into a zeroed buffer instead.
    /// The key a slot is found and rejected by.
    ///
    /// Eight bytes or fewer: the bytes themselves, which together with the
    /// length *are* the text - so a hit needs no comparison at all. Folded
    /// byte by byte rather than copied, because `copy_from_slice` with a
    /// length the compiler does not know becomes a call to `memcpy`, and that
    /// call costs more than the whole rest of a lookup (measured: 13.2 ns a
    /// lookup against 8.1).
    ///
    /// Longer: the first eight bytes mixed with the *last* eight. Only the
    /// first would make `identifier_0001` and `identifier_0002` share a tag,
    /// and every miss between such words would then pay a resolve and a
    /// comparison before interning anyway - which measured +30 ns on a miss,
    /// against the ~3 ns a miss should cost.
    #[inline]
    fn tag(text: &str) -> u64 {
        let b = text.as_bytes();
        if b.len() > 8 {
            let first = u64::from_le_bytes(b[..8].try_into().expect("eight bytes"));
            let last = u64::from_le_bytes(b[b.len() - 8..].try_into().expect("eight bytes"));
            return first ^ last.rotate_left(31);
        }
        let mut tag = 0u64;
        for (i, &c) in b.iter().enumerate() {
            tag |= (c as u64) << (i * 8);
        }
        tag
    }

    #[inline]
    fn index(&self, tag: u64) -> usize {
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
            self.interner = id;
            self.slots.clear();
        }
    }

    /// The symbol for `text`, from the cache when it is there and from the
    /// interner otherwise.
    /// Out of line and marked cold: it runs once per context that interns.
    #[cold]
    #[inline(never)]
    fn fill(&mut self) {
        self.slots = vec![Self::empty_slot(); 1 << self.bits];
    }

    #[inline]
    pub(crate) fn intern(&mut self, interner: &InternerContext, text: &str) -> Symbol {
        if self.slots.is_empty() {
            self.fill();
        }
        let tag = Self::tag(text);
        let len = text.len() as u32;
        let at = self.index(tag);
        let slot = &mut self.slots[at];

        if slot.sym != 0 && slot.tag == tag && slot.len == len {
            let sym = Symbol::from_index(slot.sym - 1);
            // Eight bytes and the length *are* the text when the text is that
            // short, so a hit needs no comparison. Longer texts share a tag
            // (`customer_id` and `customer_name` do), and are verified.
            if len <= 8 || interner.resolve(sym) == text {
                return sym;
            }
        }

        let sym = interner.intern_string(text);
        *slot = Slot {
            tag,
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
    /// A clone is *empty*. `ParseContext` is cloned once per piece of a
    /// `par_fold`, and copying eight kilobytes of cache into a piece that has
    /// not parsed anything yet would cost more than the misses it saves. A
    /// cache has no semantics to preserve.
    fn clone(&self) -> Self {
        Self::new()
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

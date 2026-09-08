//! A lookup cache in front of the interner - `TODO.md` §4.
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
/// Public only because it is a field of [`ParseContext`](crate::ParseContext),
/// which callers build with a struct literal. It has no API and no
/// guarantees; do not name it.
pub struct InternCache {
    slots: Box<[Slot]>,
    /// Which interner the slots belong to. A context whose interner is
    /// replaced between parses gets an empty cache rather than another
    /// interner's numbers.
    interner: usize,
}

impl InternCache {
    /// 512 slots, 8 KiB. L1d is 32-48 KiB *in total* and the parse also wants
    /// the input, the stack and its output in there, so the table is sized to
    /// leave room rather than to fill the cache.
    const BITS: u32 = 9;
    const SLOTS: usize = 1 << Self::BITS;

    fn empty_slot() -> Slot {
        Slot {
            tag: 0,
            len: 0,
            sym: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            slots: vec![Self::empty_slot(); Self::SLOTS].into_boxed_slice(),
            interner: 0,
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
    fn index(tag: u64) -> usize {
        // A multiply folds the low bits upward; the top `BITS` are then the
        // bucket. Fixed constant: the cache is not adversary-facing - a
        // collision costs one interner call, not a quadratic blow-up.
        (tag.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - Self::BITS)) as usize
    }

    /// Empties the cache if it belongs to another interner. Called where a
    /// parse begins, which is the only moment the interner can have changed.
    pub(crate) fn rebind(&mut self, interner: &InternerContext) {
        let id = interner.id();
        if self.interner != id {
            self.interner = id;
            self.slots.fill(Self::empty_slot());
        }
    }

    /// The symbol for `text`, from the cache when it is there and from the
    /// interner otherwise.
    #[inline]
    pub(crate) fn intern(&mut self, interner: &InternerContext, text: &str) -> Symbol {
        let tag = Self::tag(text);
        let len = text.len() as u32;
        let slot = &mut self.slots[Self::index(tag)];

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

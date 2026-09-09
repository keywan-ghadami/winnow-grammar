//! Where the time in one `intern_string` goes.
//!
//! `benches/interning.rs` measures interning in place; this one takes it
//! apart, because a change to the interner is worth making only if it targets
//! the part that dominates. Four points, from the whole thing down to the
//! floor:
//!
//! | | ns per lookup |
//! |---|---|
//! | `InternerContext` (`ThreadedRodeo`, std `RandomState`) | ~21 |
//! | `ThreadedRodeo` directly - the wrapper costs nothing | ~21 |
//! | `Rodeo`, the single-threaded one - no shard lock | ~16 |
//! | `HashMap<&str, u32>` with the same hasher - hash and probe alone | ~14 |
//! | a direct-index slot table, the 1BRC shape | **~7** |
//!
//! So of ~21 ns: **~14 ns hashing and probing, ~6 ns the dashmap shard lock,
//! ~1.5 ns lasso's own bookkeeping.** Measured on the hot case - eight
//! distinct short words, every call a hit - which is what a parse does.
//!
//! The last row is the ceiling, not a proposal: a bespoke table that hands
//! back the slot number as the identity, with no central map, no lock and
//! nothing to resolve. It is three times faster than the general interner and
//! it is a different data structure, not a tuned one. A grammar can have one:
//! `state T;` puts it in the parse's own state - see ADR 20.
//!
//! **Replacing the hasher was tried and is rejected.** Three attempts, all
//! measured here, all slower than std's `RandomState` on the hash-and-probe
//! row above: an FxHash-style hasher ~23 ns, the same with an xor-shift
//! finalizer ~25, a hand-written folded 128-bit multiply ~30, against ~14 for
//! the default. Speed here is **avalanche, not instruction count** - FxHash
//! keeps its entropy in the low bits while both the shard index and
//! hashbrown's control byte come from the top, so short keys collide and every
//! lookup pays extra string comparisons. And a hand-written hasher loses on
//! the tail: a variable-length `copy_from_slice` compiles to a call to
//! `memcpy`, which costs more than the multiplies save. Only `ahash` beat the
//! default (~6 ns), and that is a dependency decision -
//! `ThreadedRodeo::with_hasher(ahash::RandomState::new())` is the whole change
//! if it is ever wanted.
//!
//! The lookup cache in `ParseContext` took the other route and took all of it,
//! the lock included: ~9 ns a hit against ~23, and 43% off an
//! identifier-heavy parse (`benches/interning.rs`).
//!
//! Numbers are one machine's; the *shape* is the point. Re-measure before
//! deciding, not after.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use lasso::{Rodeo, ThreadedRodeo};
use std::collections::HashMap;
use std::hint::black_box;
use winnow_grammar::InternerContext;

const CITIES: [&str; 8] = [
    "Hamburg",
    "Zürich",
    "São Paulo",
    "東京",
    "Abidjan",
    "Reykjavík",
    "A",
    "Ust-Kamenogorsk",
];

/// A direct-index table of the kind a hand-written 1BRC solution uses: a
/// power-of-two array, linear probing, and the slot number handed back as the
/// identity. The caller indexes its accumulators with it directly - there is
/// no second lookup and nothing to resolve.
struct Slots {
    tags: Vec<u64>,
    names: Vec<&'static str>,
    slots: Vec<u32>,
}

impl Slots {
    const BITS: usize = 10; // 1024 buckets

    fn new() -> Self {
        Self {
            tags: vec![0; 1 << Self::BITS],
            names: Vec::new(),
            slots: vec![u32::MAX; 1 << Self::BITS],
        }
    }

    /// The first eight bytes, zero-padded, plus the length: a tag that is
    /// cheap to compare and never the identity on its own - two names can
    /// share it, so a hit is verified against the stored name.
    #[inline]
    fn tag(s: &str) -> u64 {
        let b = s.as_bytes();
        let n = b.len().min(8);
        let mut buf = [0u8; 8];
        buf[..n].copy_from_slice(&b[..n]);
        u64::from_le_bytes(buf) ^ ((b.len() as u64) << 56)
    }

    #[inline]
    fn slot(&mut self, s: &'static str) -> u32 {
        let tag = Self::tag(s);
        let mask = (1 << Self::BITS) - 1;
        let mut i = (tag.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - Self::BITS)) as usize;
        loop {
            let occupied = self.slots[i] != u32::MAX;
            if !occupied {
                let id = self.names.len() as u32;
                self.names.push(s);
                self.tags[i] = tag;
                self.slots[i] = id;
                return id;
            }
            // A name of eight bytes or fewer is *entirely* in the tag, length
            // included, so tag equality is exact and there is no string to
            // compare. Longer names fall back to the comparison.
            if self.tags[i] == tag && (s.len() <= 8 || self.names[self.slots[i] as usize] == s) {
                return self.slots[i];
            }
            i = (i + 1) & mask;
        }
    }
}

fn bench(c: &mut Criterion) {
    let words: Vec<&str> = (0..1024).map(|i| CITIES[i % CITIES.len()]).collect();
    let mut g = c.benchmark_group("where");
    g.throughput(Throughput::Elements(words.len() as u64));

    g.bench_function("InternerContext(ThreadedRodeo+SipHash)", |b| {
        let it = InternerContext::new();
        for w in &words {
            it.intern_string(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(it.intern_string(black_box(w)));
            }
        })
    });

    g.bench_function("ThreadedRodeo direct", |b| {
        let it = ThreadedRodeo::default();
        for w in &words {
            it.get_or_intern(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(it.get_or_intern(black_box(w)));
            }
        })
    });

    g.bench_function("Rodeo (single-threaded, no lock)", |b| {
        let mut it = Rodeo::default();
        for w in &words {
            it.get_or_intern(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(it.get_or_intern(black_box(w)));
            }
        })
    });

    g.bench_function("HashMap<&str,u32> (SipHash, no interner)", |b| {
        let mut m: HashMap<&str, u32> = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            m.insert(w, i as u32);
        }
        b.iter(|| {
            for w in &words {
                black_box(m.get(black_box(w as &str)));
            }
        })
    });

    // What a bespoke interner costs - the 1BRC shape, where the "symbol" *is*
    // the slot in the caller's own table: one open-addressed array, the first
    // eight bytes as the probe key, no central map, no lock, no `resolve`.
    // This is the ceiling the general interner is measured against.
    g.bench_function("direct-index slot table (1BRC shape)", |b| {
        let mut t = Slots::new();
        for w in &words {
            t.slot(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(t.slot(black_box(w)));
            }
        })
    });

    g.finish();
}
criterion_group!(b, bench);
criterion_main!(b);

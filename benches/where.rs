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
//!
//! So of ~21 ns: **~14 ns hashing and probing, ~6 ns the dashmap shard lock,
//! ~1.5 ns lasso's own bookkeeping.** Measured on the hot case - eight
//! distinct short words, every call a hit - which is what a parse does.
//!
//! Numbers are one machine's; the *shape* is the point, and it is what
//! `TODO.md` reasons from. Re-measure before deciding, not after.

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

    g.finish();
}
criterion_group!(b, bench);
criterion_main!(b);

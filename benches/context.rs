//! What a `ParseContext` costs, and why it is worth keeping one.
//!
//! This benchmark is why the interner and the cache are built on first use.
//! Before that, a `ParseContext` cost **1.35 µs**, essentially all of it
//! `InternerContext::new()` - a `ThreadedRodeo` is a sharded map and allocates
//! every shard - and a parse of `42` with a fresh context was 1.44 µs, of
//! which the parse itself was 50 ns. **A grammar that never writes `ident` or
//! `intern(…)` paid all of it.**
//!
//! Now: the interner alone is 32 ns, a context 46 ns, and that same parse
//! 59 ns. Cloning a context is still the thing to do - it shares the interner,
//! which is the point - but it is no longer 28x cheaper than building one.

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseInput};

grammar! {
    grammar Tiny {
        // A grammar that never interns: does it pay for the cache?
        pub num -> u32 = v:u32 -> { v }
    }
}

fn bench(c: &mut Criterion) {
    c.bench_function("context/one_number_fresh_context", |b| {
        b.iter(|| {
            let mut s = ParseInput {
                input: LocatingSlice::new("42"),
                state: ParseContext::<()>::default(),
            };
            black_box(Tiny::parse_num().parse_next(&mut s).unwrap())
        })
    });
    c.bench_function("context/context_alone", |b| {
        b.iter(|| black_box(ParseContext::<()>::default()))
    });
    c.bench_function("context/interner_alone", |b| {
        b.iter(|| black_box(winnow_grammar::InternerContext::new()))
    });
    // The same parse with a context the caller already has - cloned, which is
    // an `Arc` bump and three empty vectors, not a new interner.
    c.bench_function("context/one_number_cloned_context", |b| {
        let ctx = ParseContext::<()>::default();
        b.iter(|| {
            let mut s = ParseInput {
                input: LocatingSlice::new("42"),
                state: ctx.clone(),
            };
            black_box(Tiny::parse_num().parse_next(&mut s).unwrap())
        })
    });
}
criterion_group!(b, bench);
criterion_main!(b);

//! What a `ParseContext` costs, and why it is worth keeping one.
//!
//! ADR 14 says the interner is long-lived and shared. This is that advice with
//! a number: building a context is **1.4 µs**, essentially all of it
//! `InternerContext::new()` - a `ThreadedRodeo` is a sharded map, and it
//! allocates every shard. Parsing `42` with a fresh context takes 1.44 µs, of
//! which the parse is 50 ns; the same parse with a *cloned* context is 50 ns
//! flat, because a clone is an `Arc` bump and a few empty vectors.
//!
//! So: build one context, clone it. A caller who builds one per small input
//! spends 96% of the time constructing an interner they then throw away.

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

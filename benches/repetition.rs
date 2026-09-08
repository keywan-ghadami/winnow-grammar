//! What a repetition costs, split by what the grammar does with its result.
//!
//! A repetition whose result is bound has to produce it. One whose result is
//! discarded (`x*` with no binding) or only counted (`count(x)`) does not -
//! the grammar has already said so. These benchmarks are here to decide
//! whether that distinction is worth generating, and to keep the bounded
//! case (`digit{1,2}`, the 1BRC temperature) honest about what it costs.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseInput};

grammar! {
    grammar Rep {
        WS -> () = "" -> { () }

        // Bound: the action names the elements, so they have to exist.
        pub BOUND -> usize = xs:digit* -> { xs.len() }

        // Discarded: nothing names the result. Today a `Vec` is built and
        // thrown away by `.map(|_| ())`.
        pub DISCARDED -> () = digit* -> { () }

        // Counted: `count(p)` today builds a `Vec` to ask for its length.
        pub COUNTED -> usize = n:count(digit) -> { n }

        // The 1BRC temperature: a bounded repetition whose elements are used.
        pub TENTHS -> i32 =
            neg:"-"? whole:digit{1,2} "." frac:digit
            -> {
                let mut v: i32 = 0;
                for d in whole { v = v * 10 + (d as i32 - '0' as i32); }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // The pair that isolates the collection: the same pattern, once with
        // its elements named and once without.
        pub BOUNDED_BOUND -> usize = xs:digit{1,2} -> { xs.len() }
        pub BOUNDED_DISCARDED -> () = digit{1,2} -> { () }

        // The same, unbounded and short - where an allocation is not
        // amortised over thousands of elements.
        pub SHORT_BOUND -> usize = xs:digit* -> { xs.len() }
        pub SHORT_DISCARDED -> () = digit* -> { () }
        pub SHORT_COUNTED -> usize = n:count(digit) -> { n }
    }
}

/// A run of `n` digits. The rules below are lexical (capitalised), so there
/// is no implicit whitespace to separate elements with.
fn digits(n: usize) -> String {
    (0..n).map(|i| char::from(b'0' + (i % 10) as u8)).collect()
}

fn bench_repetition(c: &mut Criterion) {
    let mut g = c.benchmark_group("repetition");

    // One element per digit. Two sizes: at 2000 the collection fits in cache
    // and costs almost nothing, at 200_000 it is 800 KB of `char` that a
    // counting repetition never touches.
    let input = digits(200_000);
    g.throughput(Throughput::Bytes(input.len() as u64));

    // The context is built once and cloned per iteration - an `Arc` clone.
    // Constructing one allocates a `ThreadedRodeo`, which on a short input
    // costs more than the parse and would be all this measured.
    let ctx = ParseContext::<()>::default();
    macro_rules! case {
        ($name:expr, $parser:expr, $text:expr) => {
            g.bench_function($name, |b| {
                b.iter(|| {
                    let mut stream = ParseInput {
                        input: LocatingSlice::new($text),
                        state: ctx.clone(),
                    };
                    black_box($parser.parse_next(&mut stream).unwrap())
                })
            });
        };
    }

    case!("bound/200k", Rep::parse_BOUND(), input.as_str());
    case!("discarded/200k", Rep::parse_DISCARDED(), input.as_str());
    case!("counted/200k", Rep::parse_COUNTED(), input.as_str());

    g.finish();
}

fn bench_bounded(c: &mut Criterion) {
    let mut g = c.benchmark_group("bounded");

    // One temperature, parsed over and over: the per-item cost is the point,
    // not throughput over a large input.
    // The context is built once and cloned per iteration - an `Arc` clone.
    // Constructing one allocates a `ThreadedRodeo`, which on a short input
    // costs more than the parse and would be all this measured.
    let ctx = ParseContext::<()>::default();
    macro_rules! case {
        ($name:expr, $parser:expr, $text:expr) => {
            g.bench_function($name, |b| {
                b.iter(|| {
                    let mut stream = ParseInput {
                        input: LocatingSlice::new($text),
                        state: ctx.clone(),
                    };
                    black_box($parser.parse_next(&mut stream).unwrap())
                })
            });
        };
    }

    case!("tenths", Rep::parse_TENTHS(), "-12.3");
    case!("bound", Rep::parse_BOUNDED_BOUND(), "12");
    case!("discarded", Rep::parse_BOUNDED_DISCARDED(), "12");
    case!("short/bound", Rep::parse_SHORT_BOUND(), "12345");
    case!("short/discarded", Rep::parse_SHORT_DISCARDED(), "12345");
    case!("short/counted", Rep::parse_SHORT_COUNTED(), "12345");

    g.finish();
}

criterion_group!(benches, bench_repetition, bench_bounded);
criterion_main!(benches);

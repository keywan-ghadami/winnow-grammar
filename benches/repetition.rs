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
use winnow_grammar::error::ParseError;
use winnow_grammar::{grammar, ParseContext, ParseInput};

/// What `text(digit{1,2})` would generate: the matched run as a borrowed
/// slice instead of a `Vec<char>`. Winnow's `.take()` under another name -
/// this stands in for the operator so the design can be measured before it
/// is designed.
fn digits_1_2<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
) -> Result<&'a str, ParseError> {
    winnow::token::take_while(1..=2, |c: char| c.is_ascii_digit()).parse_next(i)
}

/// What `dec(digit{1,2})` would generate: the run accumulated straight into
/// an integer, with no slice and no fold in the action. The bound is known,
/// so nothing inside the loop can overflow an `i32`.
fn dec_1_2<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> Result<i32, ParseError> {
    let s: &str = winnow::token::take_while(1..=2, |c: char| c.is_ascii_digit()).parse_next(i)?;
    let mut v: i32 = 0;
    for &b in s.as_bytes() {
        v = v * 10 + (b - b'0') as i32;
    }
    Ok(v)
}

/// The ceiling: the same temperature read by hand, one scan and a fold.
/// Not a proposal - a number to hold the generated forms against.
fn tenths_by_hand<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
) -> Result<i32, ParseError> {
    let s: &str =
        winnow::token::take_while(1.., |c: char| c == '-' || c == '.' || c.is_ascii_digit())
            .parse_next(i)?;
    let mut b = s.as_bytes();
    let neg = b[0] == b'-';
    if neg {
        b = &b[1..];
    }
    let mut v: i32 = 0;
    for &c in b {
        if c != b'.' {
            v = v * 10 + (c - b'0') as i32;
        }
    }
    Ok(if neg { -v } else { v })
}

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

        // Where TENTHS's time goes. `FLOOR` is entry and `finish` with no
        // pattern to speak of; `TWO_DIGITS` is the same two digits without any
        // repetition machinery, so the gap to `BOUNDED_BOUND` is what the
        // bounded repetition itself costs.
        pub FLOOR -> () = "" -> { () }
        pub ONE_DIGIT -> i32 = d:digit -> { d as i32 - 48 }
        pub TWO_DIGITS -> i32 = a:digit b:digit -> { (a as i32 - 48) * 10 + (b as i32 - 48) }
        pub BY_HAND -> i32 = v:super::tenths_by_hand -> { v }

        // The same again, with the run turned into a number by the parser
        // instead of by the action - what `dec(..)` would do.
        pub TENTHS_DEC -> i32 =
            neg:"-"? whole:super::dec_1_2 "." frac:digit
            -> {
                let v = whole * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // The same rule, with only the digit run changed from `Vec<char>` to
        // the borrowed slice a text-capture operator would give it.
        pub TENTHS_TEXT -> i32 =
            neg:"-"? whole:super::digits_1_2 "." frac:digit
            -> {
                let mut v: i32 = 0;
                for &b in whole.as_bytes() { v = v * 10 + (b - b'0') as i32; }
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

/// Where the time in the 1BRC temperature goes.
///
/// Read the **differences**, not the absolute numbers: every case pays the
/// same stream construction and context clone, which on an input this short
/// is most of what the clock sees. (A case that only builds the stream and
/// `black_box`es it measures *higher* than one that parses an empty rule -
/// forcing the whole struct to memory costs more than using it - so there is
/// no honest floor to subtract, only pairs to compare.)
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

    case!("floor", Rep::parse_FLOOR(), "");
    case!("one_digit", Rep::parse_ONE_DIGIT(), "1");
    case!("two_digits", Rep::parse_TWO_DIGITS(), "12");
    case!("tenths", Rep::parse_TENTHS(), "-12.3");
    case!("tenths/via_text", Rep::parse_TENTHS_TEXT(), "-12.3");
    case!("tenths/via_dec", Rep::parse_TENTHS_DEC(), "-12.3");
    case!("tenths/by_hand", Rep::parse_BY_HAND(), "-12.3");
    case!("bound", Rep::parse_BOUNDED_BOUND(), "12");
    case!("discarded", Rep::parse_BOUNDED_DISCARDED(), "12");
    case!("short/bound", Rep::parse_SHORT_BOUND(), "12345");
    case!("short/discarded", Rep::parse_SHORT_DISCARDED(), "12345");
    case!("short/counted", Rep::parse_SHORT_COUNTED(), "12345");

    g.finish();
}

criterion_group!(benches, bench_repetition, bench_bounded);
criterion_main!(benches);

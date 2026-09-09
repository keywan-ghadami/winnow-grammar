//! What a repetition costs, split by what the grammar does with its result.
//!
//! A repetition of a character class (`digit`, `any`) is the text it matched -
//! a slice of the input, whatever the grammar does with it. A repetition of a
//! *rule* yields its elements, because those are values the parser built
//! rather than input it walked over, and one whose result is discarded or only
//! counted yields neither. These benchmarks measure the difference, and keep
//! the bounded case (`digit{1,2}`, the 1BRC temperature) honest about what it
//! costs.
//!
//! What they established, so that it is not re-derived: the repetition loop
//! itself is free - a run costs less than the same number of separate parses -
//! and **one heap allocation for two `char`s cost ~23 ns**, which was the
//! whole gap between the generated temperature and a hand-written one. Not
//! putting two characters on the heap closed it; no SIMD, no SWAR, no register
//! arithmetic. A run of a character class is therefore the text it matched,
//! and `dec<T>(p)` accumulates it without a `Vec` in between - kept for the
//! overflow bound and the ergonomics rather than for time, which it does not
//! buy.
//!
//! Read differences, not absolutes: every case pays the same stream
//! construction. And do not clone a `ParseContext` per iteration - it carries
//! the interner's 8 KiB lookup cache, which costs ~98 ns to clone and as much
//! again to drop, so a harness that does it measures itself.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::error::ParseError;
use winnow_grammar::{grammar, ParseContext, ParseInput};

/// A `take_while` scan of the same run, kept as a comparison for the
/// generated `digit{1,2}`: a closure over a character class against a
/// repetition of `one_of` wrapped in `.take()`.
fn digits_1_2<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
) -> Result<&'a str, ParseError> {
    winnow::token::take_while(1..=2, |c: char| c.is_ascii_digit()).parse_next(i)
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

        // A repetition of a *rule* collects: the elements are values the
        // parser built - one `u8` each - so the `Vec` is the answer rather
        // than a copy of input that is already there.
        ITEM -> u8 = d:digit -> { d as u8 - b'0' }
        pub COLLECTED -> usize = xs:ITEM* -> { xs.len() }

        // A repetition of a character class is the text it matched, so naming
        // it costs nothing beyond the slice.
        pub RUN -> usize = xs:digit* -> { xs.len() }

        // Nothing names the result, so nothing is built either way.
        pub DISCARDED -> () = digit* -> { () }

        // `count(p)` answers with one number and holds no elements.
        pub COUNTED -> usize = n:count(digit) -> { n }

        // The 1BRC temperature: a bounded run read out of its own text.
        pub TENTHS -> i32 =
            neg:"-"? whole:digit{1,2} "." frac:digit
            -> {
                let mut v: i32 = 0;
                for b in whole.bytes() { v = v * 10 + (b - b'0') as i32; }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // Where TENTHS's time goes. `FLOOR` is entry and `finish` with no
        // pattern to speak of; `TWO_DIGITS` is the same two digits without any
        // repetition machinery, so the gap to `BOUNDED_RUN` is what the
        // bounded repetition itself costs.
        pub FLOOR -> () = "" -> { () }
        pub ONE_DIGIT -> i32 = d:digit -> { d as i32 - 48 }
        pub TWO_DIGITS -> i32 = a:digit b:digit -> { (a as i32 - 48) * 10 + (b as i32 - 48) }
        pub BY_HAND -> i32 = v:super::tenths_by_hand -> { v }

        // The operators over the same run, so the numbers are about what is
        // generated rather than about a stand-in. `text(..)` over a run is not
        // a second wrapper - it is what the run already is.
        pub TENTHS_DEC -> i32 =
            neg:"-"? whole:dec<i32>(digit{1,2}) "." frac:dec<i32>(digit)
            -> { let v = whole * 10 + frac; if neg.is_some() { -v } else { v } }

        pub TENTHS_TEXT -> i32 =
            neg:"-"? whole:text(digit{1,2}) "." frac:digit
            -> {
                let mut v: i32 = 0;
                for &b in whole.as_bytes() { v = v * 10 + (b - b'0') as i32; }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // The stand-in kept for comparison: a `take_while` scan rather than
        // a repetition of `one_of` wrapped in `.take()`.
        pub TENTHS_SCAN -> i32 =
            neg:"-"? whole:super::digits_1_2 "." frac:digit
            -> {
                let mut v: i32 = 0;
                for &b in whole.as_bytes() { v = v * 10 + (b - b'0') as i32; }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // The pair that isolates the allocation: the same two digits as the
        // text they matched, and as elements a rule built.
        pub BOUNDED_RUN -> usize = xs:digit{1,2} -> { xs.len() }
        pub BOUNDED_COLLECTED -> usize = xs:ITEM{1,2} -> { xs.len() }
        pub BOUNDED_DISCARDED -> () = digit{1,2} -> { () }

        // The same, unbounded and short - where an allocation is not
        // amortised over thousands of elements.
        pub SHORT_RUN -> usize = xs:digit* -> { xs.len() }
        pub SHORT_COLLECTED -> usize = xs:ITEM* -> { xs.len() }
        pub SHORT_DISCARDED -> () = digit* -> { () }
        pub SHORT_COUNTED -> usize = n:count(digit) -> { n }
    }
}

/// A run of `n` digits. The rules below are lexical (capitalised), so there
/// is no implicit whitespace to separate elements with.
fn digits(n: usize) -> String {
    (0..n).map(|i| char::from(b'0' + (i % 10) as u8)).collect()
}

/// One stream, reused: a `ParseContext` is **not** cheap to clone since it
/// carries the interner's 8 KiB lookup cache (TODO.md §4), and cloning one per
/// iteration measures that allocation rather than the rule - ~140 ns, more
/// than anything in this file costs. Reusing it measures repeated parsing,
/// which is the case these rules are for; `rt::entry` clears the diagnostics
/// engine's working space at every parse, so nothing carries over.
macro_rules! bench_cases {
    ($g:expr, $ctx:expr, [$(($name:expr, $parser:expr, $text:expr)),* $(,)?]) => {
        $(
            $g.bench_function($name, |b| {
                let mut stream = ParseInput {
                    input: LocatingSlice::new($text),
                    state: $ctx.clone(),
                };
                b.iter(|| {
                    stream.input = LocatingSlice::new($text);
                    black_box($parser.parse_next(&mut stream).unwrap())
                })
            });
        )*
    };
}

fn bench_repetition(c: &mut Criterion) {
    let mut g = c.benchmark_group("repetition");

    // One element per digit, 200_000 of them: 200 KB of `u8` that a collecting
    // repetition holds and the others never touch.
    let input = digits(200_000);
    g.throughput(Throughput::Bytes(input.len() as u64));

    let ctx = ParseContext::<()>::default();
    bench_cases!(
        g,
        ctx,
        [
            ("collected/200k", Rep::parse_COLLECTED(), input.as_str()),
            ("run/200k", Rep::parse_RUN(), input.as_str()),
            ("discarded/200k", Rep::parse_DISCARDED(), input.as_str()),
            ("counted/200k", Rep::parse_COUNTED(), input.as_str()),
        ]
    );

    g.finish();
}

/// Where the time in the 1BRC temperature goes.
///
/// Read the **differences**, not the absolute numbers: every case pays the
/// same stream construction, which on an input this short is a real share of
/// what the clock sees.
fn bench_bounded(c: &mut Criterion) {
    let mut g = c.benchmark_group("bounded");

    // One temperature, parsed over and over: the per-item cost is the point,
    // not throughput over a large input.
    let ctx = ParseContext::<()>::default();
    bench_cases!(
        g,
        ctx,
        [
            ("floor", Rep::parse_FLOOR(), ""),
            ("one_digit", Rep::parse_ONE_DIGIT(), "1"),
            ("two_digits", Rep::parse_TWO_DIGITS(), "12"),
            ("tenths", Rep::parse_TENTHS(), "-12.3"),
            ("tenths/via_text", Rep::parse_TENTHS_TEXT(), "-12.3"),
            ("tenths/via_dec", Rep::parse_TENTHS_DEC(), "-12.3"),
            ("tenths/via_scan", Rep::parse_TENTHS_SCAN(), "-12.3"),
            ("tenths/by_hand", Rep::parse_BY_HAND(), "-12.3"),
            ("run", Rep::parse_BOUNDED_RUN(), "12"),
            ("collected", Rep::parse_BOUNDED_COLLECTED(), "12"),
            ("discarded", Rep::parse_BOUNDED_DISCARDED(), "12"),
            ("short/run", Rep::parse_SHORT_RUN(), "12345"),
            ("short/collected", Rep::parse_SHORT_COLLECTED(), "12345"),
            ("short/discarded", Rep::parse_SHORT_DISCARDED(), "12345"),
            ("short/counted", Rep::parse_SHORT_COUNTED(), "12345"),
        ]
    );

    g.finish();
}

criterion_group!(benches, bench_repetition, bench_bounded);
criterion_main!(benches);

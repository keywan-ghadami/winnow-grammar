//! What decoding costs where a grammar does not need it - the 1BRC trick for a
//! `&str` input: work on bytes, and decode only where a `char` is asked for.
//!
//! A `&str` is already valid UTF-8, so a scan for an ASCII byte class cannot
//! land mid-character - a continuation byte is >= 0x80 and can never satisfy
//! an ASCII predicate. The slice it cuts is therefore valid UTF-8 by
//! construction, with nothing to validate and nothing to decode.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use winnow::stream::{AsBStr, LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::ascii::AsciiClass;
use winnow_grammar::{ParseContext, ParseInput};

fn input(n: usize) -> String {
    "0123456789".repeat(n / 10)
}

/// What the generated code does today: a predicate over `char`.
fn by_char<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> &'a str {
    let r: Result<&'a str, winnow::error::ErrMode<winnow::error::EmptyError>> =
        winnow::token::take_while(1.., |c: char| c.is_ascii_digit()).parse_next(i);
    r.unwrap()
}

/// The same class, counted over bytes and cut once.
fn by_byte<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> &'a str {
    let n = i
        .as_bstr()
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    i.next_slice(n)
}

/// What ships: the same word-at-a-time scan, through the class table the
/// generated code uses (`AsciiClass`, `src/ascii.rs`). Its mask is a hair
/// wider than the hand-written digit test - it costs two subtractions per
/// range instead of one subtraction and one addition, because a per-byte
/// comparison has to stop the borrow rather than guard against it - so this
/// is the honest number, not the best case.
fn by_word<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> &'a str {
    let n = AsciiClass::DIGIT.run(i.as_bstr());
    i.next_slice(n)
}

/// The realistic shape: many *short* runs, which is what a data format has -
/// a 1BRC temperature is one or two digits. One iteration walks the whole
/// input, alternating a digit run with the separator after it, so the
/// per-run cost is what is measured rather than the stream construction.
fn walk<'a>(input: &'a str, scan: fn(&mut ParseInput<'a, ()>) -> &'a str) -> usize {
    let mut i = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    let mut n = 0;
    while !i.as_bstr().is_empty() {
        n += scan(&mut i).len();
        if !i.as_bstr().is_empty() {
            let _ = i.next_slice(1); // the ';'
        }
    }
    n
}

fn bench_short(c: &mut Criterion) {
    let mut g = c.benchmark_group("deferred_short");
    for run in [2usize, 7, 20] {
        let s: String =
            std::iter::repeat_n(format!("{};", "1".repeat(run)), 20_000).collect::<String>();
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(format!("by_char/run{run}"), &s, |b, s| {
            b.iter(|| black_box(walk(s, by_char)))
        });
        g.bench_with_input(format!("by_word/run{run}"), &s, |b, s| {
            b.iter(|| black_box(walk(s, by_word)))
        });
    }
    g.finish();
}

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("deferred");
    for len in [16usize, 200_000] {
        let s = input(len);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(format!("by_char/{len}"), &s, |b, s| {
            b.iter(|| {
                let mut i = ParseInput {
                    input: LocatingSlice::new(s.as_str()),
                    state: ParseContext::<()>::default(),
                };
                black_box(by_char(&mut i).len())
            })
        });
        g.bench_with_input(format!("by_word/{len}"), &s, |b, s| {
            b.iter(|| {
                let mut i = ParseInput {
                    input: LocatingSlice::new(s.as_str()),
                    state: ParseContext::<()>::default(),
                };
                black_box(by_word(&mut i).len())
            })
        });
        g.bench_with_input(format!("by_byte/{len}"), &s, |b, s| {
            b.iter(|| {
                let mut i = ParseInput {
                    input: LocatingSlice::new(s.as_str()),
                    state: ParseContext::<()>::default(),
                };
                black_box(by_byte(&mut i).len())
            })
        });
    }
    g.finish();
}

criterion_group!(b, bench, bench_short);
criterion_main!(b);

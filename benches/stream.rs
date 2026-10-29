//! What reading from a stream costs (ADR 27).
//!
//! * `edges` - a grammar with a `#[stream]` rule wraps every parser in
//!   `rt::edge` and gives up the indexed runs of ADR 24 §8. The same rules
//!   parsed in memory, with and without, say what that costs a grammar that
//!   streams when it parses an input it has whole.
//! * `partial` - what ADR 27 §2 asked: winnow's `Partial<&str>` with
//!   `partial = false` against `&str`, for a parser of winnow's own. The
//!   answer decided nothing in the end - §3 was built without `Partial` - but
//!   it is the number the question was about.
//! * `reader` - a reader against the parse in memory, the input in a
//!   `Cursor`: the price of the steps (`#[stream]`) and of the blocks
//!   (`par_fold`), and of the reads alone, which is what overlapping reading
//!   with parsing could at most win.
//!
//! The numbers are in ADR 27 §5.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::io::{Cursor, Read};
use winnow::stream::{LocatingSlice, Partial, StreamIsPartial};
use winnow::Parser;
use winnow_grammar::rt::{Parallelism, Reading};
use winnow_grammar::{grammar, ParseContext, ParseInput};

grammar! {
    grammar Plain {
        #[frame(boundary = "\n")]
        pub ROW -> u64 = until(";" | frame_end) ";" v:u64 frame_end -> { v }
        pub FILE -> u64 = s:par_fold(ROW, || 0u64, |a: u64, v: u64| a + v, |a: u64, b: u64| a + b) -> { s }

        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }

        pub list -> i64 = "[" s:fold(num, || 0i64, |a: i64, n: i64| a + n) "]" -> { s }
        num -> i64 = n:i64 ","? -> { n }
    }
}

grammar! {
    grammar Tracked {
        #[stream]
        pub ROWS -> u64 = s:fold(LINE, || 0u64, |a: u64, v: u64| a + v) -> { s }
        LINE -> u64 = until(";" | "\n") ";" v:u64 "\n" -> { v }

        #[stream]
        pub list -> i64 = "[" s:fold(num, || 0i64, |a: i64, n: i64| a + n) "]" -> { s }
        num -> i64 = n:i64 ","? -> { n }
    }
}

fn rows(n: usize) -> String {
    (0..n)
        .map(|i| format!("station{};{}\n", i % 413, (i * 37) % 1000))
        .collect()
}

fn list(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| ((i as i64 * 37) % 2001 - 1000).to_string())
        .collect();
    format!("[ {} ]", items.join(", "))
}

fn in_memory<'a, T>(
    p: &mut impl Parser<ParseInput<'a, ()>, T, winnow_grammar::ParseError>,
    input: &'a str,
) -> T {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    })
    .unwrap()
}

fn edges(c: &mut Criterion) {
    let rows = rows(100_000);
    let list = list(100_000);
    let mut g = c.benchmark_group("edges");
    g.throughput(Throughput::Bytes(rows.len() as u64));
    g.bench_function("rows/plain", |b| {
        b.iter(|| in_memory(&mut Plain::parse_ROWS(), black_box(&rows)))
    });
    g.bench_function("rows/tracked", |b| {
        b.iter(|| in_memory(&mut Tracked::parse_ROWS(), black_box(&rows)))
    });
    g.throughput(Throughput::Bytes(list.len() as u64));
    g.bench_function("list/plain", |b| {
        b.iter(|| in_memory(&mut Plain::parse_list(), black_box(&list)))
    });
    g.bench_function("list/tracked", |b| {
        b.iter(|| in_memory(&mut Tracked::parse_list(), black_box(&list)))
    });
    g.finish();
}

/// `name;digits\n` rows summed, by winnow alone, over any stream.
fn winnow_rows<I>(input: &mut I) -> winnow::Result<u64>
where
    I: winnow::stream::Stream<Token = char>
        + winnow::stream::StreamIsPartial
        + winnow::stream::Compare<char>
        + winnow::stream::FindSlice<char>,
    <I as winnow::stream::Stream>::Slice: AsRef<str>,
{
    use winnow::combinator::{repeat, terminated};
    use winnow::token::{take_till, take_while};
    repeat(
        0..,
        terminated(
            (
                take_till(1.., ';'),
                ';',
                take_while(1.., |c: char| c.is_ascii_digit()).map(
                    |d: <I as winnow::stream::Stream>::Slice| d.as_ref().parse::<u64>().unwrap(),
                ),
            ),
            '\n',
        )
        .map(|(_, _, v)| v),
    )
    .fold(|| 0u64, |a, v| a + v)
    .parse_next(input)
}

fn partial(c: &mut Criterion) {
    let rows = rows(100_000);
    let mut g = c.benchmark_group("partial");
    g.throughput(Throughput::Bytes(rows.len() as u64));
    g.bench_function("str", |b| {
        b.iter(|| {
            let mut i = black_box(rows.as_str());
            winnow_rows(&mut i).unwrap()
        })
    });
    g.bench_function("partial_complete", |b| {
        b.iter(|| {
            let mut i = Partial::new(black_box(rows.as_str()));
            let _ = i.complete();
            winnow_rows(&mut i).unwrap()
        })
    });
    g.finish();
}

fn reader(c: &mut Criterion) {
    let rows = rows(400_000);
    let list = list(200_000);
    let reading = Reading::default();
    let mut g = c.benchmark_group("reader");
    g.throughput(Throughput::Bytes(rows.len() as u64));
    g.bench_function("rows/memory", |b| {
        b.iter(|| in_memory(&mut Tracked::parse_ROWS(), black_box(&rows)))
    });
    g.bench_function("rows/stream", |b| {
        b.iter(|| {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_ROWS_reader(Cursor::new(black_box(rows.as_bytes())), &mut ctx, reading)
                .unwrap()
        })
    });
    g.bench_function("rows/frames_memory", |b| {
        b.iter(|| {
            Plain::parse_FILE_pieces(
                black_box(&rows),
                &ParseContext::<()>::default(),
                Parallelism::Off,
            )
            .unwrap()
        })
    });
    g.bench_function("rows/frames_reader", |b| {
        b.iter(|| {
            Plain::parse_FILE_reader(
                Cursor::new(black_box(rows.as_bytes())),
                &ParseContext::<()>::default(),
                Parallelism::Off,
                reading,
            )
            .unwrap()
        })
    });
    g.bench_function("rows/read_only", |b| {
        b.iter(|| {
            let mut r = Cursor::new(black_box(rows.as_bytes()));
            let mut buf = vec![0u8; reading.block];
            let mut n = 0;
            loop {
                let k = r.read(&mut buf).unwrap();
                if k == 0 {
                    break n;
                }
                n += k;
            }
        })
    });
    g.throughput(Throughput::Bytes(list.len() as u64));
    g.bench_function("list/memory", |b| {
        b.iter(|| in_memory(&mut Tracked::parse_list(), black_box(&list)))
    });
    g.bench_function("list/stream", |b| {
        b.iter(|| {
            let mut ctx = ParseContext::<()>::default();
            Tracked::parse_list_reader(Cursor::new(black_box(list.as_bytes())), &mut ctx, reading)
                .unwrap()
        })
    });
    for block in [4 << 10, 64 << 10] {
        g.bench_with_input(
            BenchmarkId::new("list/stream_block", block),
            &block,
            |b, &block| {
                b.iter(|| {
                    let mut ctx = ParseContext::<()>::default();
                    Tracked::parse_list_reader(
                        Cursor::new(black_box(list.as_bytes())),
                        &mut ctx,
                        Reading { block, ..reading },
                    )
                    .unwrap()
                })
            },
        );
    }
    g.finish();
}

criterion_group!(benches, edges, partial, reader);
criterion_main!(benches);

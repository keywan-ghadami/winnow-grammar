//! What a cut `=>` costs (ADR 15, *a cut decides*).
//!
//! Each statement `let vN = N;` is parsed by an item rule with a cut after
//! `let` and by the same rule without it, in memory - as a rule's own
//! sequence (statements) and inside a repetition (combinators) - and through
//! `#[stream]`, where the stream rule's own prefix holds a cut that spans
//! the fold. The cut's work: `rt::commit` pushes an entry on the context's
//! stash, the sequence's scope pops it, and the trivia after the cut is
//! skipped with `rt::skip_trivia_cut`.
//!
//! The numbers are in ADR 15.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::hint::black_box;
use std::io::Cursor;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, ParseContext, ParseInput};

grammar! {
    grammar Cuts {
        pub stmts -> usize = n:fold(stmt, || 0usize, |n: usize, _s: ()| n + 1) -> { n }
        stmt -> () = "let" => _n:raw_ident "=" _v:u32 ";" -> { () }

        pub grouped -> usize = s:("let" => raw_ident "=" u32 ";")* -> { s.len() }

        #[stream]
        pub doc -> usize = "[" => n:fold(stmt, || 0usize, |n: usize, _s: ()| n + 1) "]" -> { n }
    }
}

grammar! {
    grammar NoCuts {
        pub stmts -> usize = n:fold(stmt, || 0usize, |n: usize, _s: ()| n + 1) -> { n }
        stmt -> () = "let" _n:raw_ident "=" _v:u32 ";" -> { () }

        pub grouped -> usize = s:("let" raw_ident "=" u32 ";")* -> { s.len() }

        #[stream]
        pub doc -> usize = "[" n:fold(stmt, || 0usize, |n: usize, _s: ()| n + 1) "]" -> { n }
    }
}

const N: usize = 50_000;

fn stmts() -> String {
    (0..N).map(|i| format!("let v{i} = {i};\n")).collect()
}

fn memory<'a, O>(
    mut p: impl Parser<ParseInput<'a, ()>, O, winnow_grammar::ParseError>,
    text: &'a str,
) -> O {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(text),
        state: ParseContext::default(),
    })
    .unwrap()
}

fn cut(c: &mut Criterion) {
    let text = stmts();
    let doc = format!("[{text}]");
    let reading = Reading::default();
    let mut g = c.benchmark_group("cut");
    g.throughput(Throughput::Elements(N as u64));

    g.bench_function("stmts/cut", |b| {
        b.iter(|| assert_eq!(memory(Cuts::parse_stmts(), black_box(&text)), N))
    });
    g.bench_function("stmts/none", |b| {
        b.iter(|| assert_eq!(memory(NoCuts::parse_stmts(), black_box(&text)), N))
    });
    g.bench_function("grouped/cut", |b| {
        b.iter(|| assert_eq!(memory(Cuts::parse_grouped(), black_box(&text)), N))
    });
    g.bench_function("grouped/none", |b| {
        b.iter(|| assert_eq!(memory(NoCuts::parse_grouped(), black_box(&text)), N))
    });
    g.bench_function("stream/cut", |b| {
        b.iter(|| {
            let mut ctx = ParseContext::<()>::default();
            let n =
                Cuts::parse_doc_reader(Cursor::new(black_box(doc.as_bytes())), &mut ctx, reading)
                    .unwrap();
            assert_eq!(n, N)
        })
    });
    g.bench_function("stream/none", |b| {
        b.iter(|| {
            let mut ctx = ParseContext::<()>::default();
            let n =
                NoCuts::parse_doc_reader(Cursor::new(black_box(doc.as_bytes())), &mut ctx, reading)
                    .unwrap();
            assert_eq!(n, N)
        })
    });
    g.finish();
}

criterion_group!(benches, cut);
criterion_main!(benches);

//! What interning costs, and where.
//!
//! Three levels, because a change to the interner is only visible at the level
//! it acts on:
//!
//! * `interner/*` - `intern_string` alone, no parser. This is where a hasher
//!   or a lookup cache shows its full effect; everything above dilutes it.
//! * `parse/idents` - an identifier-heavy grammar: the ratio the interner has
//!   to the rest of a parse in the case it was built for.
//! * `parse/rows` - the 1BRC shape, `intern(until(";"))` over a data format,
//!   sequential and cut into pieces. This is the case `intern` opened up, and
//!   the one where repetition is highest.
//!
//! Run one level: `cargo bench --bench interning -- interner`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Parallelism;
use winnow_grammar::{grammar, InternerContext, ParseContext, ParseInput, Symbol};

// -----------------------------------------------------------------------------
// The grammars
// -----------------------------------------------------------------------------

grammar! {
    grammar Idents {
        // A statement list: `name = other_name;`. Two idents per statement,
        // drawn from a small vocabulary - what source code looks like to an
        // interner.
        pub program -> Vec<(Symbol, Symbol)> = ss:stmt* -> { ss }

        rule stmt -> (Symbol, Symbol) = a:ident "=" b:ident ";" -> { (a, b) }
    }
}

grammar! {
    grammar Rows {
        WS -> () = "" -> { () }

        #[frame(boundary = "\n")]
        pub ROW -> (Symbol, i32) =
            c:intern(until(";" | frame_end)) ";" t:i32 frame_end -> { (c, t) }

        pub FILE -> usize = n:par_fold(
            ROW,
            || 0usize,
            |acc: usize, _row: (Symbol, i32)| acc + 1,
            |a: usize, b: usize| a + b
        ) -> { n }
    }
}

// -----------------------------------------------------------------------------
// Inputs
// -----------------------------------------------------------------------------

/// A small vocabulary, as a program has: the same names over and over.
const NAMES: [&str; 12] = [
    "counter", "total", "index", "value", "buffer", "result", "offset", "length", "state", "input",
    "output", "tmp",
];

/// Cities as the 1BRC file has them - short, non-ASCII, and repeating.
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

fn program(statements: usize) -> String {
    let mut s = String::new();
    for i in 0..statements {
        s.push_str(NAMES[(i * 7) % NAMES.len()]);
        s.push_str(" = ");
        s.push_str(NAMES[(i * 5 + 3) % NAMES.len()]);
        s.push_str(";\n");
    }
    s
}

fn rows(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let tenths = ((i as i64 * 37) % 1999) as i32 - 999;
        s.push_str(CITIES[(i * 7) % CITIES.len()]);
        s.push(';');
        s.push_str(&tenths.to_string());
        s.push('\n');
    }
    s
}

// -----------------------------------------------------------------------------
// interner/* - `intern_string` with no parser around it
// -----------------------------------------------------------------------------

fn bench_interner(c: &mut Criterion) {
    let mut g = c.benchmark_group("interner");

    // The hot case: a handful of distinct strings, hit again and again. Every
    // call hashes and looks up; none of them inserts after the first pass.
    let words: Vec<&str> = (0..1024).map(|i| CITIES[i % CITIES.len()]).collect();
    g.throughput(Throughput::Elements(words.len() as u64));
    g.bench_function("hot_set_1024_lookups_8_distinct", |b| {
        let interner = InternerContext::new();
        for w in &words {
            interner.intern_string(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(interner.intern_string(black_box(w)));
            }
        })
    });

    // The cold case: every string is new, so every call inserts. This is the
    // half a lookup cache cannot help with.
    let fresh: Vec<String> = (0..1024).map(|i| format!("identifier_{i}")).collect();
    g.throughput(Throughput::Elements(fresh.len() as u64));
    g.bench_function("cold_1024_distinct_inserts", |b| {
        b.iter_batched_ref(
            InternerContext::new,
            |interner| {
                for w in &fresh {
                    black_box(interner.intern_string(black_box(w)));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Long strings: hashing cost grows with length, lookup cost does not.
    let long: Vec<String> = (0..256)
        .map(|i| format!("a_rather_long_identifier_name_number_{}", i % 8))
        .collect();
    g.throughput(Throughput::Elements(long.len() as u64));
    g.bench_function("hot_set_256_lookups_long_strings", |b| {
        let interner = InternerContext::new();
        for w in &long {
            interner.intern_string(w);
        }
        b.iter(|| {
            for w in &long {
                black_box(interner.intern_string(black_box(w)));
            }
        })
    });

    // The same two cases through `ParseContext::intern`, which is what a parse
    // calls: the cache in front of the interner (`src/intern_cache.rs`). The pair above
    // is the control - it goes to the interner directly.
    g.throughput(Throughput::Elements(words.len() as u64));
    g.bench_function("cached_hot_set_1024_lookups_8_distinct", |b| {
        let mut ctx = ParseContext::<()>::default();
        for w in &words {
            ctx.intern(w);
        }
        b.iter(|| {
            for w in &words {
                black_box(ctx.intern(black_box(w)));
            }
        })
    });

    // The verify path: longer than eight bytes, so a hit is confirmed against
    // the interned text before it is believed.
    g.throughput(Throughput::Elements(long.len() as u64));
    g.bench_function("cached_hot_set_256_lookups_long_strings", |b| {
        let mut ctx = ParseContext::<()>::default();
        for w in &long {
            ctx.intern(w);
        }
        b.iter(|| {
            for w in &long {
                black_box(ctx.intern(black_box(w)));
            }
        })
    });

    // Batched, because building the context allocates the cache and that is
    // setup, not the miss path being measured.
    g.throughput(Throughput::Elements(fresh.len() as u64));
    g.bench_function("cached_cold_1024_distinct_inserts", |b| {
        b.iter_batched_ref(
            ParseContext::<()>::default,
            |ctx| {
                for w in &fresh {
                    black_box(ctx.intern(black_box(w)));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    g.finish();
}

// -----------------------------------------------------------------------------
// parse/* - the interner inside a parse
// -----------------------------------------------------------------------------

fn bench_parse_idents(c: &mut Criterion) {
    let mut g = c.benchmark_group("parse");
    for statements in [100usize, 2000] {
        let input = program(statements);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("idents", statements),
            &input,
            |b, input| {
                b.iter(|| {
                    let mut stream = ParseInput {
                        input: LocatingSlice::new(input.as_str()),
                        state: ParseContext::<()>::default(),
                    };
                    let out = Idents::parse_program().parse_next(&mut stream).unwrap();
                    black_box(out.len())
                })
            },
        );
    }
    g.finish();
}

fn bench_parse_rows(c: &mut Criterion) {
    let mut g = c.benchmark_group("parse");
    let input = rows(5000);
    g.throughput(Throughput::Bytes(input.len() as u64));

    g.bench_function("rows/sequential", |b| {
        b.iter(|| {
            let mut stream = ParseInput {
                input: LocatingSlice::new(input.as_str()),
                state: ParseContext::<()>::default(),
            };
            black_box(Rows::parse_FILE().parse_next(&mut stream).unwrap())
        })
    });

    // Cut into pieces, all sharing one interner - the shape SYNTAX.md
    // documents. Without the `rayon` feature the pieces run in sequence, so
    // this measures the cut and the shared interner, not the parallelism.
    for pieces in [2usize, 8] {
        g.bench_with_input(
            BenchmarkId::new("rows/pieces_shared_interner", pieces),
            &pieces,
            |b, &pieces| {
                b.iter(|| {
                    let context = ParseContext::<()> {
                        interner: InternerContext::new(),
                        ..Default::default()
                    };
                    black_box(
                        Rows::parse_FILE_pieces(
                            input.as_str(),
                            &context,
                            Parallelism::Pieces(pieces),
                        )
                        .unwrap(),
                    )
                })
            },
        );
    }

    g.finish();
}

criterion_group!(
    benches,
    bench_interner,
    bench_parse_idents,
    bench_parse_rows
);
criterion_main!(benches);

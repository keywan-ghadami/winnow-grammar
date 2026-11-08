//! The callgrind half of `benches/cut.rs`: one parse of 200 000 statements
//! `let vN = N;`, chosen by the first argument, so that instructions per
//! statement can be counted. `scripts/profile-cut.sh` runs every case and
//! subtracts `none`, the cost of building the input. The numbers are in
//! ADR 15.
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `stmts_cut` / `stmts_none` | an item rule with and without a cut after `let`, in memory |
//! | `grouped_cut` / `grouped_none` | the same sequence inside a repetition (combinators) |
//! | `stream_cut` / `stream_none` | a `#[stream]` reader, `[` … `]` around the items, with and without a cut after `[` (`Diagnose::Replay`) |

use std::io::Cursor;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, ParseContext, ParseInput};

pub const N: usize = 200_000;

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

fn main() {
    let text: String = (0..N).map(|i| format!("let v{i} = {i};\n")).collect();
    let doc = format!("[{text}]");
    let case = std::env::args().nth(1).unwrap_or_default();
    let reader = |cut: bool| {
        let mut ctx = ParseContext::<()>::default();
        let r = Cursor::new(doc.as_bytes());
        if cut {
            Cuts::parse_doc_reader(r, &mut ctx, Reading::default()).unwrap()
        } else {
            NoCuts::parse_doc_reader(r, &mut ctx, Reading::default()).unwrap()
        }
    };
    let n = match case.as_str() {
        "none" => N,
        "stmts_cut" => memory(Cuts::parse_stmts(), &text),
        "stmts_none" => memory(NoCuts::parse_stmts(), &text),
        "grouped_cut" => memory(Cuts::parse_grouped(), &text),
        "grouped_none" => memory(NoCuts::parse_grouped(), &text),
        "stream_cut" => reader(true),
        "stream_none" => reader(false),
        _ => {
            eprintln!("usage: cut_profile none|stmts_cut|stmts_none|grouped_cut|grouped_none|stream_cut|stream_none");
            std::process::exit(2);
        }
    };
    assert_eq!(n, N);
    println!("{n}");
}

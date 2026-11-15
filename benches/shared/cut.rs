//! The grammars `benches/cut.rs` measures and `examples/cut_profile.rs`
//! counts instructions of - one file, so that the profile and the benchmark
//! cannot drift apart. Each includes it with `#[path]`.

use winnow::stream::LocatingSlice;
use winnow::Parser;
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

/// `n` statements `let vN = N;`, one per line.
pub fn stmts(n: usize) -> String {
    (0..n).map(|i| format!("let v{i} = {i};\n")).collect()
}

pub fn memory<'a, O>(
    mut p: impl Parser<ParseInput<'a, ()>, O, winnow_grammar::ParseError>,
    text: &'a str,
) -> O {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(text),
        state: ParseContext::default(),
    })
    .unwrap()
}

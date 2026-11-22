//! Spans bound with `@` in the parse in memory, for callgrind: 200 000
//! words, each bound with its span, chosen by the first argument. In a
//! grammar with a `#[stream]` rule a span is moved by the context's
//! `origin` (0 in memory), so that a stream's steps and the parse in memory
//! agree on positions; `plain` and `dual` set the two side by side (#43).
//!
//! | case | what runs |
//! |---|---|
//! | `none` | nothing: the input is built and dropped |
//! | `plain` | the words in memory, in a grammar without `#[stream]` |
//! | `dual` | the same rules in memory, in a grammar with a `#[stream]` rule |

use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseInput};

pub const N: usize = 200_000;

grammar! {
    grammar Plain {
        pub words -> usize = n:fold(word, || 0usize, |n: usize, s: usize| n + s) -> { n }
        word -> usize = w:ident_text @ at -> { (at.start & 7) + w.len() }
        ident_text -> &'a str = w:raw_ident -> { w }
    }
}

grammar! {
    grammar Dual {
        pub words -> usize = n:fold(word, || 0usize, |n: usize, s: usize| n + s) -> { n }
        word -> usize = w:ident_text @ at -> { (at.start & 7) + w.len() }
        ident_text -> &'a str = w:raw_ident -> { w }

        #[stream]
        pub streamed -> usize = n:fold(word, || 0usize, |n: usize, s: usize| n + s) -> { n }
    }
}

fn main() {
    let text = "alpha beta gamma delta ".repeat(N / 4);
    let case = std::env::args().nth(1).unwrap_or_default();
    let mut i = ParseInput {
        input: LocatingSlice::new(text.as_str()),
        state: ParseContext::<()>::default(),
    };
    let n = match case.as_str() {
        "none" => 0,
        "plain" => Plain::parse_words().parse_next(&mut i).unwrap(),
        "dual" => Dual::parse_words().parse_next(&mut i).unwrap(),
        _ => {
            eprintln!("usage: span_profile none|plain|dual");
            std::process::exit(2);
        }
    };
    println!("{n}");
}

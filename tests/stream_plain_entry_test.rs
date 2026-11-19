//! A grammar with a `#[stream]` rule is generated twice (ADR 27 §3, *Per-mode
//! copies*): the stream reader runs the copy that notes where a parser
//! depended on the end of its input, every plain entry the copy that does
//! not. A plain parse in such a grammar gives the value the grammar gives
//! without its stream rule, and leaves the edge state - `touched`, `reach` -
//! as it found it.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Mixed {
        #[stream]
        pub DOC -> usize = n:fold(ITEM, || 0usize, |n: usize, i: usize| n + i) eof -> { n }
        pub LIST -> usize = n:fold(ITEM, || 0usize, |n: usize, i: usize| n + i) eof -> { n }
        ITEM -> usize = "abc" "!" -> { 10 } | "ab" -> { 1 } | string -> { 100 }
    }
}

grammar! {
    // The same rules without the stream rule: what a plain parse must give.
    grammar Plain {
        pub LIST -> usize = n:fold(ITEM, || 0usize, |n: usize, i: usize| n + i) eof -> { n }
        ITEM -> usize = "abc" "!" -> { 10 } | "ab" -> { 1 } | string -> { 100 }
    }
}

const INPUTS: [&str; 7] = [
    "",
    "ab",
    "ab abc!",
    "abc! \"q\" ab",
    "abc",
    "ab \"open",
    "abx",
];

/// Parses `input` in memory and hands back the result and the context.
fn parse<'a>(
    p: &mut impl Parser<ParseInput<'a, ()>, usize, ParseError>,
    input: &'a str,
) -> (Result<usize, ParseError>, ParseContext<()>) {
    let mut i = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    let r = p.parse_next(&mut i);
    (r, i.state)
}

fn same(a: &Result<usize, ParseError>, b: &Result<usize, ParseError>, what: &str) {
    match (a, b) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}"),
        (Err(a), Err(b)) => {
            assert_eq!(a.offset, b.offset, "{what}: offsets");
            assert_eq!(a.headline(), b.headline(), "{what}: messages");
        }
        _ => panic!("{what}:\n {a:?}\n {b:?}"),
    }
}

#[test]
fn a_plain_entry_gives_the_grammars_value_without_the_stream_rule() {
    for input in INPUTS {
        let (want, _) = parse(&mut Plain::parse_LIST(), input);
        let (list, _) = parse(&mut Mixed::parse_LIST(), input);
        same(&want, &list, &format!("LIST {input:?}"));
        // The stream rule's own plain entry is a plain parse too.
        let (doc, _) = parse(&mut Mixed::parse_DOC(), input);
        same(&want, &doc, &format!("DOC {input:?}"));
    }
}

#[test]
fn a_plain_entry_leaves_the_edge_state_alone() {
    // Every input ends within the grammar's lookahead of where a parser
    // stops or fails: the tracked copy would set `touched` on each, and
    // `reach` on those that fail. `scripts/test-edges.sh` generates every
    // grammar as one tracked copy, on purpose: there is nothing to check.
    if option_env!("WINNOW_GRAMMAR_EDGES").is_some() {
        return;
    }
    for input in INPUTS {
        for (what, ctx) in [
            ("LIST", parse(&mut Mixed::parse_LIST(), input).1),
            ("DOC", parse(&mut Mixed::parse_DOC(), input).1),
        ] {
            assert!(!ctx.touched, "{what} {input:?}: touched");
            assert_eq!(ctx.reach, usize::MAX, "{what} {input:?}: reach");
            assert_eq!(ctx.origin, 0, "{what} {input:?}: origin");
        }
    }
}

struct Trickle<'d>(&'d [u8]);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.0.len()).min(1);
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn the_reader_still_runs_the_tracked_copy() {
    // A byte at a time, every step first ends inside an item: only the
    // tracked copy keeps the reader's answer the in-memory one.
    for input in INPUTS {
        let (want, _) = parse(&mut Plain::parse_LIST(), input);
        let mut ctx = ParseContext::<()>::default();
        let got = Mixed::parse_DOC_reader(
            Trickle(input.as_bytes()),
            &mut ctx,
            Reading {
                block: 16,
                max_frame: 256,
            },
        );
        same(&want, &got, &format!("reader {input:?}"));
    }
}

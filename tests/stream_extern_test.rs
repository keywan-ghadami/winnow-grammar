//! A hand-written parser in a grammar with a `#[stream]` rule (ADR 27 §3) -
//! what `SYNTAX.md` asks of one, shown to be needed.
//!
//! * A scan whose failure is decided far from where it began - a block
//!   comment that is not closed - must call `rt::touch` when it reached the
//!   end of its input. Without it, a comment that is cut by the end of a block
//!   reads as no comment at all, and the reader answers what the parse in
//!   memory does not.
//! * A position taken by hand is one in the step's input; `rt::span_at` moves
//!   it to the stream's.

use std::io::Read;
use winnow::stream::{LocatingSlice, Location, Stream};
use winnow::Parser;
use winnow_grammar::rt::{self, Reading};
use winnow_grammar::{grammar, ParseContext, ParseError, ParseInput};

/// `/* … */`, its text. Fails where it began when there is no `*/`.
fn scan<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
    touch: bool,
) -> Result<&'a str, ParseError> {
    let rest: &str = &i.input;
    let fail =
        |i: &ParseInput<'a, S>| ParseError::from_stream(&i.input).add_expected("a closed comment");
    let Some(body) = rest.strip_prefix("/*") else {
        return Err(fail(i));
    };
    match body.find("*/") {
        Some(n) => Ok(i.next_slice(2 + n + 2)),
        None => {
            // Looked to the end of the input and found no `*/`: more input
            // might have held it.
            if touch {
                rt::touch(i);
            }
            Err(fail(i))
        }
    }
}

pub fn comment<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
) -> Result<&'a str, ParseError> {
    scan(i, true)
}

pub fn comment_untouched<'a, S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'a, S>,
) -> Result<&'a str, ParseError> {
    scan(i, false)
}

/// Where the parser is, in the step's input.
pub fn here<S: Clone + std::fmt::Debug>(i: &mut ParseInput<'_, S>) -> Result<usize, ParseError> {
    Ok(i.current_token_start())
}

/// Where the parser is, in the stream's.
pub fn here_moved<S: Clone + std::fmt::Debug>(
    i: &mut ParseInput<'_, S>,
) -> Result<usize, ParseError> {
    let at = i.current_token_start();
    Ok(rt::span_at(i, at..at).start)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Comment(String),
    Word(String),
}

grammar! {
    grammar Touched {
        use super::{comment, Item};
        extern rule comment -> &'a str;

        #[stream]
        pub doc -> Vec<Item> =
            v:fold(item, Vec::new, |mut v: Vec<Item>, i: Item| { v.push(i); v }) eof -> { v }
        item -> Item =
            c:comment -> { Item::Comment(c.to_string()) }
          | w:text(ident) -> { Item::Word(w.to_string()) }
    }
}

grammar! {
    grammar Untouched {
        use super::{comment_untouched, Item};
        extern rule comment_untouched -> &'a str;

        #[stream]
        pub doc -> Vec<Item> =
            v:fold(item, Vec::new, |mut v: Vec<Item>, i: Item| { v.push(i); v }) eof -> { v }
        item -> Item =
            c:comment_untouched -> { Item::Comment(c.to_string()) }
          | w:text(ident) -> { Item::Word(w.to_string()) }
    }
}

grammar! {
    grammar Positions {
        use super::{here, here_moved};
        extern rule here -> usize;
        extern rule here_moved -> usize;

        #[stream]
        pub doc -> Vec<(usize, usize, usize)> =
            v:fold(item, Vec::new, |mut v: Vec<(usize, usize, usize)>, i: (usize, usize, usize)| { v.push(i); v })
            eof -> { v }
        item -> (usize, usize, usize) = a:here b:here_moved w:ident @ s -> { let _ = w; (a, b, s.start) }
    }
}

struct Trickle<'d>(&'d [u8], usize);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.1.min(buf.len()).min(self.0.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

fn in_memory<'a, T>(
    p: &mut impl Parser<ParseInput<'a, ()>, T, ParseError>,
    input: &'a str,
) -> Result<T, ParseError> {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    })
}

const INPUT: &str = "alpha /* one */ beta /* a longer comment */ gamma";

#[test]
fn a_scan_that_calls_touch_agrees_with_the_parse_in_memory() {
    let mem = in_memory(&mut Touched::parse_doc(), INPUT).unwrap();
    assert_eq!(mem.len(), 5);
    for step in [1, 2, 3, 7, 1 << 20] {
        let mut ctx = ParseContext::<()>::default();
        let got = Touched::parse_doc_reader(
            Trickle(INPUT.as_bytes(), step),
            &mut ctx,
            Reading::default(),
        );
        assert_eq!(got.as_ref(), Ok(&mem), "step {step}");
    }
}

#[test]
fn a_scan_that_does_not_call_touch_answers_wrong_when_a_block_cuts_it() {
    let mem = in_memory(&mut Untouched::parse_doc(), INPUT).unwrap();
    assert_eq!(mem.len(), 5);
    // All at once, the end of the block is the end of the input: right.
    let mut ctx = ParseContext::<()>::default();
    let whole = Untouched::parse_doc_reader(
        Trickle(INPUT.as_bytes(), 1 << 20),
        &mut ctx,
        Reading::default(),
    );
    assert_eq!(whole.as_ref(), Ok(&mem));
    // A byte at a time, the first comment is cut, read as none, and the
    // fold ends there: an error the input does not have.
    let mut ctx = ParseContext::<()>::default();
    let cut =
        Untouched::parse_doc_reader(Trickle(INPUT.as_bytes(), 1), &mut ctx, Reading::default());
    assert!(cut.is_err(), "{cut:?}");
}

#[test]
fn a_position_taken_by_hand_is_moved_by_span_at() {
    let input = "aa bbb cccc ddddd";
    let mem = in_memory(&mut Positions::parse_doc(), input).unwrap();
    // In memory the step's input is the whole input: all three agree.
    assert!(mem.iter().all(|&(a, b, s)| a == b && b == s), "{mem:?}");
    let mut ctx = ParseContext::<()>::default();
    let got =
        Positions::parse_doc_reader(Trickle(input.as_bytes(), 1), &mut ctx, Reading::default())
            .unwrap();
    let moved: Vec<_> = got.iter().map(|&(_, b, s)| (b, s)).collect();
    let want: Vec<_> = mem.iter().map(|&(_, b, s)| (b, s)).collect();
    assert_eq!(moved, want, "span_at and `@` give the stream's positions");
    // The raw position is the step's: past the first item it is not the
    // stream's.
    assert!(got.iter().any(|&(a, b, _)| a != b), "{got:?}");
}

//! A scan's cut at its hit (#23): the text up to the hit is consumed without
//! the look back for `\r` where there is no `line_ending`, and with a compare
//! of the hit's byte where LLVM can see it. The claim is that nothing
//! observable changes: every scan below exists twice, once with up to three
//! alternatives - `memchr` and the changed cut - and once with two more that
//! never occur, which takes the position-by-position path. Both are run on
//! every string of up to six characters, a multi-byte one and `\r\n`
//! included, and must agree on the value, on how much was consumed, and on
//! the rendered error.

use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Scans {
        pub ONE -> &'a str = s:until(";") ";" -> { s }
        pub ONE_REF -> &'a str = s:until(";" | "\u{1}" | "\u{2}" | "\u{3}") ";" -> { s }

        pub TWO -> &'a str = s:until(";" | "\n") -> { s }
        pub TWO_REF -> &'a str = s:until(";" | "\n" | "\u{1}" | "\u{2}") -> { s }

        pub LINE -> &'a str = s:until(";" | line_ending) -> { s }
        pub LINE_REF -> &'a str = s:until(";" | line_ending | "\u{1}" | "\u{2}") -> { s }
    }
}

fn run<'a, O, P>(mut p: P, input: &'a str) -> (Result<O, String>, usize)
where
    P: Parser<ParseInput<'a, ()>, O, ParseError>,
{
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    // The rule's own name is the one thing the two spellings may not share.
    let result = p
        .parse_next(&mut stream)
        .map_err(|e| e.render(input).replace("_REF", ""));
    (result, input.len() - stream.eof_offset())
}

fn strings(alphabet: &[char], len: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut layer = vec![String::new()];
    for _ in 0..len {
        let mut next = Vec::with_capacity(layer.len() * alphabet.len());
        for s in &layer {
            for c in alphabet {
                let mut t = s.clone();
                t.push(*c);
                next.push(t);
            }
        }
        out.extend(next.iter().cloned());
        layer = next;
    }
    out
}

const ALPHABET: [char; 5] = ['a', ';', '\n', '\r', 'é'];

macro_rules! agree {
    ($fast:ident, $reference:ident) => {
        for s in strings(&ALPHABET, 6) {
            assert_eq!(
                run(Scans::$fast(), &s),
                run(Scans::$reference(), &s),
                "{} and {} disagree on {s:?}",
                stringify!($fast),
                stringify!($reference)
            );
        }
    };
}

#[test]
fn a_scan_for_one_needle_agrees() {
    agree!(parse_ONE, parse_ONE_REF);
}

#[test]
fn a_scan_for_two_needles_agrees() {
    agree!(parse_TWO, parse_TWO_REF);
}

#[test]
fn a_scan_with_line_ending_agrees() {
    agree!(parse_LINE, parse_LINE_REF);
}

#[test]
fn the_cut_is_where_the_hit_is() {
    // A rule reached from outside consumes its whole input, so the hit is
    // the last thing in it or there is none.
    assert_eq!(run(Scans::parse_ONE(), "é;").0, Ok("é"));
    assert_eq!(run(Scans::parse_ONE(), "a\r\n;").0, Ok("a\r\n"));
    assert_eq!(run(Scans::parse_LINE(), "ab\rc").0, Ok("ab\rc"));
    assert_eq!(run(Scans::parse_TWO(), "éé").0, Ok("éé"));
}

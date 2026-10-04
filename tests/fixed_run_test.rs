//! A run of fixed-shape elements in a lexical rule is matched by index
//! (ADR 24 §8). The claim is that nothing observable changes: every rule
//! below exists twice, once as written and once with an empty literal between
//! its elements, which matches everywhere and breaks every run - so the
//! second is generated the way the first was before. Both are run on every
//! string up to six characters over an alphabet that hits each element's
//! edge, and must agree on the value, on how much was consumed, and on the
//! rendered error.

use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::{grammar, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Fixed {
        // The 1BRC temperature.
        pub TENTHS -> (Option<&'a str>, &'a str, char) =
            neg:"-"? whole:digit{1,2} "." frac:digit -> { (neg, whole, frac) }
        pub TENTHS_REF -> (Option<&'a str>, &'a str, char) =
            neg:"-"? "" whole:digit{1,2} "" "." "" frac:digit -> { (neg, whole, frac) }

        // Nothing bound: the run is matched and nothing is kept.
        pub BARE -> bool = "-"? digit{1,2} "." digit -> { true }
        pub BARE_REF -> bool = "-"? "" digit{1,2} "" "." "" digit -> { true }

        // A char literal, a literal that is not ASCII, an optional digit and
        // an exact count.
        pub MIXED -> (&'a str, Option<char>, &'a str, &'a str) =
            a:'é' b:digit? c:digit{2} d:"x" -> { (a, b, c, d) }
        pub MIXED_REF -> (&'a str, Option<char>, &'a str, &'a str) =
            a:'é' "" b:digit? "" c:digit{2} "" d:"x" -> { (a, b, c, d) }

        // A run after a commit point: a failure in it is a cut in both.
        pub CUT -> (&'a str, char) = ";" => w:digit{1,2} "." f:digit -> { (w, f) }
        pub CUT_REF -> (&'a str, char) = ";" => w:digit{1,2} "" "." "" f:digit -> { (w, f) }

        // A run that may match nothing at all, followed by something else.
        pub EMPTY -> (Option<&'a str>, Option<char>, &'a str) =
            a:"-"? b:digit? c:raw_ident -> { (a, b, c) }
        pub EMPTY_REF -> (Option<&'a str>, Option<char>, &'a str) =
            a:"-"? "" b:digit? "" c:raw_ident -> { (a, b, c) }
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

/// Every string of up to `len` characters over `alphabet`.
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

const ALPHABET: [char; 7] = ['-', '0', '9', '.', 'x', 'é', ';'];

macro_rules! agree {
    ($fast:ident, $reference:ident) => {
        for s in strings(&ALPHABET, 6) {
            let fast = run(Fixed::$fast(), &s);
            let reference = run(Fixed::$reference(), &s);
            assert_eq!(
                fast,
                reference,
                "{} and {} disagree on {s:?}",
                stringify!($fast),
                stringify!($reference)
            );
        }
    };
}

#[test]
fn tenths_agrees_with_the_element_by_element_parse() {
    agree!(parse_TENTHS, parse_TENTHS_REF);
}

#[test]
fn an_unbound_run_agrees() {
    agree!(parse_BARE, parse_BARE_REF);
}

#[test]
fn char_and_non_ascii_literals_optional_and_exact_digits_agree() {
    agree!(parse_MIXED, parse_MIXED_REF);
}

#[test]
fn a_run_after_a_commit_point_agrees() {
    agree!(parse_CUT, parse_CUT_REF);
}

#[test]
fn a_run_that_matches_nothing_agrees() {
    agree!(parse_EMPTY, parse_EMPTY_REF);
}

#[test]
fn the_values_are_the_elements_own() {
    assert_eq!(
        run(Fixed::parse_TENTHS(), "-12.3").0,
        Ok((Some("-"), "12", '3'))
    );
    assert_eq!(run(Fixed::parse_TENTHS(), "4.5").0, Ok((None, "4", '5')));
    // Three digits: the bound stops at two, and the "." is not there.
    assert!(run(Fixed::parse_TENTHS(), "123.0").0.is_err());
    // `digit?` is greedy and does not give its digit back to `digit{2}`.
    assert_eq!(
        run(Fixed::parse_MIXED(), "é123x").0,
        Ok(("é", Some('1'), "23", "x"))
    );
    assert!(run(Fixed::parse_MIXED(), "é12x").0.is_err());
}

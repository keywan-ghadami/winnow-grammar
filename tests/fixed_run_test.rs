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

        // `dec<T>` over digits that always fit: the value is accumulated in
        // the indexed match (ADR 24 §8). The 1BRC temperature in tenths.
        pub DEC -> i32 =
            neg:"-"? w:dec<i32>(digit{1,2}) "." f:dec<i32>(digit{1}) -> {
                let v = w * 10 + f;
                if neg.is_some() { -v } else { v }
            }
        pub DEC_REF -> i32 =
            neg:"-"? "" w:dec<i32>(digit{1,2}) "" "." "" f:dec<i32>(digit{1}) -> {
                let v = w * 10 + f;
                if neg.is_some() { -v } else { v }
            }

        // Two digits always fit a `u8`; three do not, so `dec<u8>(digit{3})`
        // keeps `rt::dec` and its overflow error ("999").
        pub DEC_U8 -> (u8, u8) = a:dec<u8>(digit{2}) "-" b:dec<u8>(digit{1,3}) -> { (a, b) }
        pub DEC_U8_REF -> (u8, u8) = a:dec<u8>(digit{2}) "" "-" "" b:dec<u8>(digit{1,3}) -> { (a, b) }

        // The 1BRC line's tail as one run - separator, sign, `dec` of a
        // bounded field, point, digit, terminator - after a scan that may
        // stop just after a two-byte character, and behind a commit point.
        pub TAIL -> (&'a str, i32) =
            n:until(";" | "x") ";" => neg:"-"? w:dec<i32>(digit{1,2}) "." f:digit "x" -> {
                let v = w * 10 + (f as i32 - '0' as i32);
                (n, if neg.is_some() { -v } else { v })
            }
        pub TAIL_REF -> (&'a str, i32) =
            n:until(";" | "x") ";" => neg:"-"? "" w:dec<i32>(digit{1,2}) "" "." "" f:digit "" "x" -> {
                let v = w * 10 + (f as i32 - '0' as i32);
                (n, if neg.is_some() { -v } else { v })
            }

        // A run that ends in a literal that is not ASCII: the advance after it
        // is the checked one.
        pub WIDE_END -> (char, &'a str) = d:digit e:'é' -> { (d, e) }
        pub WIDE_END_REF -> (char, &'a str) = d:digit "" e:'é' -> { (d, e) }

        // `dec` at the widest bound that still fits, and one past it.
        pub DEC_I64 -> i64 = "+" v:dec<i64>(digit{1,18}) -> { v }
        pub DEC_U64 -> u64 = "+" v:dec<u64>(digit{1,20}) -> { v }
    }
}

grammar! {
    grammar FixedB {
        input bytes;
        pub TAIL -> (&'a [u8], i32) =
            n:until(";" | "x") ";" => neg:"-"? w:dec<i32>(digit{1,2}) "." f:digit "x" -> {
                let v = w * 10 + (f as i32 - '0' as i32);
                (n, if neg.is_some() { -v } else { v })
            }
    }
}

fn run_bytes<'a, O, P>(mut p: P, input: &'a [u8]) -> (Result<O, String>, usize)
where
    P: Parser<winnow_grammar::ParseBytes<'a, ()>, O, ParseError>,
{
    let mut stream = winnow_grammar::ParseBytes {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    let result = p.parse_next(&mut stream).map_err(|e| e.render(input));
    (result, input.len() - stream.eof_offset())
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
fn dec_of_digits_that_fit_agrees() {
    agree!(parse_DEC, parse_DEC_REF);
}

#[test]
fn dec_of_digits_that_may_not_fit_agrees() {
    agree!(parse_DEC_U8, parse_DEC_U8_REF);
}

#[test]
fn the_1brc_tail_agrees() {
    agree!(parse_TAIL, parse_TAIL_REF);
}

#[test]
fn a_run_ending_in_a_wide_literal_agrees() {
    agree!(parse_WIDE_END, parse_WIDE_END_REF);
}

#[test]
fn the_1brc_tail_yields_and_fails_where_the_elements_do() {
    assert_eq!(run(Fixed::parse_TAIL(), "Hä;-12.3x").0, Ok(("Hä", -123)));
    assert_eq!(run(Fixed::parse_TAIL(), "ä;4.5x"), (Ok(("ä", 45)), 7));
    // Every place the fixed match can fail, with the message and the offset
    // of the element-by-element parse.
    for s in [
        "ä;",
        "ä;-",
        "ä;-x",
        "ä;1",
        "ä;12",
        "ä;123.4x",
        "ä;1.",
        "ä;1.x",
        "ä;1.2",
        "ä;1.2;",
        "ä;--1.2x",
        "ä;1.2xx",
        "äx",
    ] {
        let fast = run(Fixed::parse_TAIL(), s);
        assert!(fast.0.is_err(), "{s:?}: {fast:?}");
        assert_eq!(fast, run(Fixed::parse_TAIL_REF(), s), "{s:?}");
    }
}

#[test]
fn dec_at_the_widest_bound_that_fits() {
    assert_eq!(
        run(Fixed::parse_DEC_I64(), "+999999999999999999").0,
        Ok(999_999_999_999_999_999)
    );
    // Twenty digits may not fit a `u64`: `rt::dec` reads them, and says so.
    assert_eq!(
        run(Fixed::parse_DEC_U64(), "+18446744073709551615").0,
        Ok(u64::MAX)
    );
    let overflow = run(Fixed::parse_DEC_U64(), "+18446744073709551616").0;
    assert!(
        overflow.as_ref().is_err_and(|e| e.contains("too large")),
        "{overflow:?}"
    );
}

#[test]
fn dec_yields_the_number() {
    assert_eq!(run(Fixed::parse_DEC(), "-12.3").0, Ok(-123));
    assert_eq!(run(Fixed::parse_DEC(), "07.5").0, Ok(75));
    assert_eq!(run(Fixed::parse_DEC_U8(), "42-255").0, Ok((42, 255)));
    let overflow = run(Fixed::parse_DEC_U8(), "42-999").0;
    assert!(
        overflow.as_ref().is_err_and(|e| e.contains("too large")),
        "{overflow:?}"
    );
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

#[test]
fn the_1brc_tail_over_bytes_agrees_with_text() {
    for s in strings(&ALPHABET, 6) {
        let text = run(Fixed::parse_TAIL(), &s);
        let bytes = run_bytes(FixedB::parse_TAIL(), s.as_bytes());
        assert_eq!(bytes.1, text.1, "{s:?}");
        assert_eq!(
            bytes.0.map(|(n, v)| (n.to_vec(), v)),
            text.0.map(|(n, v)| (n.as_bytes().to_vec(), v)),
            "{s:?}"
        );
    }
}

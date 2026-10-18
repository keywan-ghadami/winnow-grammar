//! A scan's cut at its hit (#23): the text up to the hit is consumed without
//! the look back for `\r` where there is no `line_ending`, and with a compare
//! of the hit's byte where LLVM can see it. The claim is that nothing
//! observable changes: every scan below exists twice, once with up to three
//! alternatives - `memchr` and the changed cut - and once with two more that
//! never occur, which takes the position-by-position path. Both are run on
//! every string of up to six characters, a multi-byte one and `\r\n`
//! included, and must agree on the value, on how much was consumed, and on
//! the rendered error.
//!
//! A scan whose needles are all one byte keeps memchr's searcher in a
//! `static` of its own (ADR 24 §12b) and cuts at the hit unchecked (§12a):
//! `TWO`, `THREE`, `LINE` and `LINE3` take that path, here and over bytes, on
//! the short strings and on long ones that reach the vector searcher, against
//! the position-by-position reference.

use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::{grammar, ParseBytes, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Scans {
        pub ONE -> &'a str = s:until(";") ";" -> { s }
        pub ONE_REF -> &'a str = s:until(";" | "\u{1}" | "\u{2}" | "\u{3}") ";" -> { s }

        pub TWO -> &'a str = s:until(";" | "\n") -> { s }
        pub TWO_REF -> &'a str = s:until(";" | "\n" | "\u{1}" | "\u{2}") -> { s }

        pub LINE -> &'a str = s:until(";" | line_ending) -> { s }
        pub LINE_REF -> &'a str = s:until(";" | line_ending | "\u{1}" | "\u{2}") -> { s }

        pub THREE -> &'a str = s:until(";" | "," | "\n") -> { s }
        pub THREE_REF -> &'a str = s:until(";" | "," | "\n" | "\u{1}") -> { s }

        pub LINE3 -> &'a str = s:until(";" | "," | line_ending) -> { s }
        pub LINE3_REF -> &'a str = s:until(";" | "," | line_ending | "\u{1}") -> { s }

        // What follows a scan: the separator, cut after the hit.
        pub FIELD -> (&'a str, &'a str) = a:until(";" | "\n") ";" b:until(";" | "\n") -> { (a, b) }
        pub FIELD_REF -> (&'a str, &'a str) =
            a:until(";" | "\n" | "\u{1}" | "\u{2}") ";" b:until(";" | "\n" | "\u{1}" | "\u{2}") -> { (a, b) }
    }
}

grammar! {
    grammar ScansB {
        input bytes;
        pub TWO -> &'a [u8] = s:until(";" | "\n") -> { s }
        pub THREE -> &'a [u8] = s:until(";" | "," | "\n") -> { s }
        pub LINE -> &'a [u8] = s:until(";" | line_ending) -> { s }
        pub LINE3 -> &'a [u8] = s:until(";" | "," | line_ending) -> { s }
        pub FIELD -> (&'a [u8], &'a [u8]) = a:until(";" | "\n") ";" b:until(";" | "\n") -> { (a, b) }
    }
}

fn run_bytes<'a, O, P>(mut p: P, input: &'a [u8]) -> (Result<O, String>, usize)
where
    P: Parser<ParseBytes<'a, ()>, O, ParseError>,
{
    let mut stream = ParseBytes {
        input: LocatingSlice::new(input),
        state: ParseContext::<()>::default(),
    };
    let result = p.parse_next(&mut stream).map_err(|e| e.render(input));
    (result, input.len() - stream.eof_offset())
}

/// Longer strings than `strings` makes, long enough for memchr's vector
/// searcher (32 bytes and more), from a fixed pseudo-random sequence: mostly
/// `a`, with the needles, `\r` and a two-byte character among them.
fn long_strings() -> Vec<String> {
    let mut seed = 0x2545_f491_u32;
    let mut out = Vec::new();
    for len in 0..140 {
        for _ in 0..8 {
            let mut s = String::new();
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                s.push(match seed % 64 {
                    0 => ';',
                    1 => '\n',
                    2 => '\r',
                    3 => ',',
                    4..=7 => 'é',
                    _ => 'a',
                });
            }
            out.push(s);
        }
    }
    out
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

#[test]
fn a_scan_for_three_needles_agrees() {
    agree!(parse_THREE, parse_THREE_REF);
}

#[test]
fn a_scan_for_two_needles_and_line_ending_agrees() {
    agree!(parse_LINE3, parse_LINE3_REF);
}

#[test]
fn a_separator_after_the_cut_agrees() {
    agree!(parse_FIELD, parse_FIELD_REF);
}

macro_rules! agree_long {
    ($($fast:ident, $reference:ident;)*) => {
        for s in long_strings() {
            $(
                let text = run(Scans::$fast(), &s);
                assert_eq!(text, run(Scans::$reference(), &s), "{} on {s:?}", stringify!($fast));
                // The same scan over bytes: the same cut, the same error.
                let bytes = run_bytes(ScansB::$fast(), s.as_bytes());
                assert_eq!(bytes.1, text.1, "{} over bytes on {s:?}", stringify!($fast));
                assert_eq!(bytes.0.is_ok(), text.0.is_ok(), "{} over bytes on {s:?}", stringify!($fast));
                assert_eq!(bytes.0.err(), text.0.err(), "{} over bytes on {s:?}", stringify!($fast));
            )*
        }
    };
}

#[test]
fn long_inputs_reach_the_vector_searcher_and_agree() {
    agree_long! {
        parse_TWO, parse_TWO_REF;
        parse_THREE, parse_THREE_REF;
        parse_LINE, parse_LINE_REF;
        parse_LINE3, parse_LINE3_REF;
        parse_FIELD, parse_FIELD_REF;
    }
}

#[test]
fn a_byte_scan_yields_what_the_text_scan_does() {
    for s in strings(&ALPHABET, 5) {
        let text = run(Scans::parse_FIELD(), &s);
        let bytes = run_bytes(ScansB::parse_FIELD(), s.as_bytes());
        assert_eq!(
            bytes.0.map(|(a, b)| (a.to_vec(), b.to_vec())),
            text.0
                .map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec())),
            "{s:?}"
        );
        assert_eq!(bytes.1, text.1, "{s:?}");
    }
}

#[test]
fn the_static_searcher_cuts_at_the_hit() {
    // A hit at the start, at the end, none, and a two-byte character right
    // before the separator.
    assert_eq!(run(Scans::parse_FIELD(), ";b").0, Ok(("", "b")));
    assert_eq!(run(Scans::parse_FIELD(), "a;").0, Ok(("a", "")));
    assert_eq!(run(Scans::parse_TWO(), "abä").0, Ok("abä"));
    assert_eq!(run(Scans::parse_FIELD(), "Hambä;ä").0, Ok(("Hambä", "ä")));
    assert_eq!(run(Scans::parse_THREE(), "ä").0, Ok("ä"));
    assert_eq!(
        run_bytes(ScansB::parse_FIELD(), "ä;b".as_bytes())
            .0
            .map(|(a, _)| a),
        Ok("ä".as_bytes())
    );
    assert_eq!(
        run_bytes(ScansB::parse_FIELD(), b"\xc3;\xff").0,
        Ok((&b"\xc3"[..], &b"\xff"[..]))
    );
}

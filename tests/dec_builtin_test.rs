//! The integer built-ins (`u8` … `usize`, `i8` … `isize`) are
//! `rt::dec_uint` and `rt::dec_int`, no longer winnow's `dec_uint` and
//! `dec_int`, which since winnow 1.0 check as UTF-8 what they matched as ASCII
//! digits. The claim is that nothing a grammar can see changes but the message
//! of a value too large for its type: on every string of up to four
//! characters over an alphabet that hits each edge - signs, `0`, the digits
//! where `u8` and `i8` overflow, a letter, a character that is not ASCII - both
//! give the same value, consume the same, fail at the same offset, leave the
//! input in the same place and fail the same way (backtrack or cut). Over
//! bytes the alphabet adds a byte that is not UTF-8 and part of a character.
//!
//! And through a grammar: where there is no number, the next alternative is
//! tried, also when what is there is a character that is not ASCII or, over
//! bytes, not UTF-8 at all.

use winnow::error::ErrMode;
use winnow::stream::{LocatingSlice, Location};
use winnow::Parser;
use winnow_grammar::rt::{self, Input, Slice};
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseContext, ParseError};

/// What a parse did, in the terms a grammar can observe.
#[derive(Debug, PartialEq)]
enum Seen<T> {
    Ok { value: T, at: usize },
    Backtrack { error_at: usize, input_at: usize },
    Cut { error_at: usize, input_at: usize },
}

/// Whether a failure says why: only the new one does, and only on overflow.
fn says_why(e: &ParseError) -> bool {
    e.message.is_some()
}

fn run<B: Slice, T, P>(p: P, slice: B) -> (Seen<T>, Option<ParseError>)
where
    P: Parser<Input<B, ()>, T, ErrMode<ParseError>>,
{
    let mut i = Input {
        input: LocatingSlice::new(slice),
        state: ParseContext::<()>::default(),
    };
    let at = |i: &Input<B, ()>| i.current_token_start();
    // What the generated code calls: the built-in with its expectation.
    let mut p = rt::expected("integer literal", p);
    match p.parse_next(&mut i) {
        Ok(value) => (Seen::Ok { value, at: at(&i) }, None),
        Err(ErrMode::Backtrack(e)) => (
            Seen::Backtrack {
                error_at: e.offset,
                input_at: at(&i),
            },
            Some(e),
        ),
        Err(ErrMode::Cut(e)) => (
            Seen::Cut {
                error_at: e.offset,
                input_at: at(&i),
            },
            Some(e),
        ),
        Err(ErrMode::Incomplete(_)) => unreachable!("the input is complete"),
    }
}

/// An integer built-in, new or old.
type Dec<B, T> = fn(&mut Input<B, ()>) -> Result<T, ErrMode<ParseError>>;

/// The new and the old parser agree on `slice`.
fn agree<B: Slice + Copy + AsRef<[u8]>, T: PartialEq + std::fmt::Debug + std::str::FromStr>(
    new: Dec<B, T>,
    old: Dec<B, T>,
    slice: B,
) {
    let (n, ne) = run(new, slice);
    let (o, oe) = run(old, slice);
    assert_eq!(n, o, "on {slice:?}");
    match (ne, oe) {
        // The new one says why only where a number was there and did not
        // fit; every other failure renders as the old one did.
        (Some(ne), Some(oe)) if says_why(&ne) => {
            let m = ne.message.as_deref().unwrap();
            assert!(
                m.contains("too large") || m.contains("too small"),
                "{m} on {slice:?}"
            );
            assert_eq!(ne.offset, oe.offset, "on {slice:?}");
        }
        (Some(ne), Some(oe)) => {
            assert_eq!(
                ne.render(slice.as_ref()),
                oe.render(slice.as_ref()),
                "on {slice:?}"
            )
        }
        (None, None) => {}
        (ne, oe) => panic!("{ne:?} / {oe:?} on {slice:?}"),
    }
}

/// Every string up to `len` items over `alphabet`.
fn strings<T: Clone>(alphabet: &[T], len: usize) -> Vec<Vec<T>> {
    let mut all = vec![vec![]];
    let mut last = vec![vec![]];
    for _ in 0..len {
        last = last
            .iter()
            .flat_map(|s: &Vec<T>| {
                alphabet.iter().map(move |c| {
                    let mut s = s.clone();
                    s.push(c.clone());
                    s
                })
            })
            .collect();
        all.extend(last.iter().cloned());
    }
    all
}

const TEXT: &[&str] = &["0", "1", "2", "5", "6", "8", "9", "+", "-", "x", "é"];

macro_rules! text_cases {
    ($($f:ident $t:ty),*) => {{
        let all = strings(TEXT, 4);
        for s in &all {
            let s = s.concat();
            $(
                agree::<&str, $t>(
                    rt::$f::<_, (), $t, ParseError>,
                    winnow::ascii::$f::<Input<&str, ()>, $t, ErrMode<ParseError>>,
                    s.as_str(),
                );
            )*
        }
        all.len()
    }};
}

macro_rules! byte_cases {
    ($($f:ident $t:ty),*) => {{
        // `0xc3` begins `é` and is cut off here; `0xa9` is the rest of it
        // without its start; `0xff` is never UTF-8.
        let alphabet: &[&[u8]] = &[b"0", b"1", b"2", b"5", b"6", b"9", b"+", b"-", b"x", &[0xc3], &[0xa9], &[0xff]];
        let all = strings(alphabet, 4);
        for s in &all {
            let s = s.concat();
            $(
                agree::<&[u8], $t>(
                    rt::$f::<_, (), $t, ParseError>,
                    winnow::ascii::$f::<Input<&[u8], ()>, $t, ErrMode<ParseError>>,
                    s.as_slice(),
                );
            )*
        }
        all.len()
    }};
}

#[test]
fn over_text_the_built_ins_do_what_winnows_did() {
    let n =
        text_cases!(dec_uint u8, dec_uint u16, dec_uint u64, dec_int i8, dec_int i16, dec_int i64);
    assert!(n > 10_000);
}

#[test]
fn over_bytes_the_built_ins_do_what_winnows_did() {
    let n = byte_cases!(dec_uint u8, dec_uint u32, dec_int i8, dec_int i32);
    assert!(n > 20_000);
}

#[test]
fn the_longest_values_and_one_past_them() {
    for s in [
        "255",
        "256",
        "-128",
        "-129",
        "127",
        "128",
        "+127",
        "+128",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211456",
        "00",
        "-0",
        "+0",
        "-01",
        "0123",
    ] {
        agree::<&str, u8>(
            rt::dec_uint::<_, (), u8, ParseError>,
            winnow::ascii::dec_uint,
            s,
        );
        agree::<&str, i8>(
            rt::dec_int::<_, (), i8, ParseError>,
            winnow::ascii::dec_int,
            s,
        );
        agree::<&str, u64>(
            rt::dec_uint::<_, (), u64, ParseError>,
            winnow::ascii::dec_uint,
            s,
        );
        agree::<&str, i64>(
            rt::dec_int::<_, (), i64, ParseError>,
            winnow::ascii::dec_int,
            s,
        );
        agree::<&str, u128>(
            rt::dec_uint::<_, (), u128, ParseError>,
            winnow::ascii::dec_uint,
            s,
        );
        agree::<&str, i128>(
            rt::dec_int::<_, (), i128, ParseError>,
            winnow::ascii::dec_int,
            s,
        );
    }
}

grammar! {
    grammar Text {
        // Where there is no number the second alternative is tried.
        pub either -> &'a str = i32 -> { "int" } | until(eof) -> { "other" }
        pub tail -> (i32, &'a str) = v:i32 r:until(eof) -> { (v, r) }
        pub small -> u8 = v:u8 -> { v }
    }
}

grammar! {
    grammar Bytes {
        input bytes;
        pub either -> &'a str = i32 -> { "int" } | byte* -> { "byte" }
        // An alternative that makes text of what is there is where a bad
        // byte is an error, and only there.
        pub as_text -> &'a str = i32 -> { "int" } | t:text(byte) -> { t }
    }
}

#[test]
fn where_there_is_no_number_the_next_alternative_is_tried() {
    for s in ["é", "東", "x", "-é", "+x", "-0", "+07"] {
        Text::parse_either()
            .parse_test(s)
            .assert_success_is("other");
    }
    Text::parse_either()
        .parse_test("12")
        .assert_success_is("int");
    Text::parse_tail()
        .parse_test("12東")
        .assert_success_is((12, "東"));

    // `0xff` is never UTF-8, `0xa9` is the end of a character without its
    // start, `0xe6 0x9d` the start without its end.
    for s in [&[0xff][..], &[0xa9], &[0xe6, 0x9d], &[b'-', 0xff]] {
        let mut i = winnow_grammar::ParseBytes {
            input: LocatingSlice::new(s),
            state: ParseContext::<()>::default(),
        };
        assert_eq!(
            Bytes::parse_either().parse_next(&mut i).ok(),
            Some("byte"),
            "{s:?}"
        );
    }

    // Making text of `0xff` is the hard error, at its byte - not the number.
    let mut i = winnow_grammar::ParseBytes {
        input: LocatingSlice::new(&[0xff][..]),
        state: ParseContext::<()>::default(),
    };
    match Bytes::parse_as_text().parse_next(&mut i) {
        Err(e) => assert_eq!(e.invalid_utf8, Some(0xff)),
        Ok(t) => panic!("{t:?}"),
    }
}

#[test]
fn a_value_too_large_says_so() {
    Text::parse_small().parse_test("255").assert_success_is(255);
    Text::parse_small()
        .parse_test("256")
        .assert_failure_contains("too large");
}

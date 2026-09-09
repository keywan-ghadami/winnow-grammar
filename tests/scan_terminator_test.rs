//! A terminator that is a rule of your own.
//!
//! `until(…)` and `recover(…)` scan for a terminator whose match is a fixed
//! string and try any other one at every position. A rule that matches nothing
//! but literals is now the literals it matches, on both sides of the compiler:
//! the code generator scans for it, and the frame check reasons about it.
//!
//! Only a **lexical** rule qualifies. A syntactic rule skips whitespace before
//! its elements, so it does not begin where its literal does - scanning would
//! stop in a different place, and under a frame the whitespace could consume
//! the boundary. These tests pin both halves of that.

use winnow_grammar::rt::Parallelism;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, ParseContext};

grammar! {
    grammar Named {
        WS -> () = "" -> { () }

        SEP -> () = ";" -> { () }
        // A chain, and an alternation: both resolve.
        ALIAS -> usize = SEP -> { 1 }
        EITHER -> usize = ";" -> { 1 } | "|" -> { 2 }

        pub field -> &'a str = s:until(SEP) SEP digit1 -> { s }
        pub chained -> &'a str = s:until(ALIAS) ALIAS digit1 -> { s }
        pub either -> &'a str = s:until(EITHER) EITHER digit1 -> { s }
    }
}

#[test]
fn a_named_terminator_finds_the_same_end_a_literal_does() {
    Named::parse_field()
        .parse_test("Hamburg;12")
        .assert_success_is("Hamburg");
    Named::parse_chained()
        .parse_test("Hamburg;12")
        .assert_success_is("Hamburg");
    Named::parse_either()
        .parse_test("Hamburg|12")
        .assert_success_is("Hamburg");
    Named::parse_either()
        .parse_test("Hamburg;12")
        .assert_success_is("Hamburg");
}

// -----------------------------------------------------------------------------
// The frame check: a named terminator used to be rejected outright, because
// the check could not see that the group covered the boundary.
// -----------------------------------------------------------------------------

grammar! {
    grammar Framed {
        WS -> () = "" -> { () }

        SEP -> () = ";" -> { () }
        // `until(SEP | frame_end)`: the boundary is right there as an
        // alternative, and this did not compile before §8b.
        #[frame(boundary = "\n")]
        pub ROW -> &'a str = n:until(SEP | frame_end) ";" digit1 frame_end -> { n }

        pub FILE -> usize = n:par_fold(
            ROW,
            || 0usize,
            |acc: usize, name: &str| acc + name.len(),
            |a: usize, b: usize| a + b
        ) -> { n }
    }
}

#[test]
fn a_named_terminator_is_accepted_under_a_frame() {
    Framed::parse_FILE()
        .parse_test("Hamburg;12\nZurich;20\n")
        .assert_success_is("Hamburg".len() + "Zurich".len());
}

#[test]
fn the_pieces_agree_with_the_whole() {
    // The frame check's promise: cutting changes nothing. If the generator and
    // the check disagreed about where `until(SEP | frame_end)` stops, this is
    // where it would show.
    let input = "Hamburg;12\nZurich;20\nOslo;3\nBern;9\n";
    let whole = "Hamburg".len() + "Zurich".len() + "Oslo".len() + "Bern".len();
    for pieces in [1usize, 2, 3, 4] {
        let got = Framed::parse_FILE_pieces(
            input,
            &ParseContext::<()>::default(),
            Parallelism::Pieces(pieces),
        )
        .unwrap();
        assert_eq!(got, whole, "pieces = {pieces}");
    }
}

// -----------------------------------------------------------------------------
// The condition that keeps this correct: only a lexical rule is its literal.
// -----------------------------------------------------------------------------

grammar! {
    grammar Whitespace {
        // The default `WS` (multispace0) applies: `sep` is syntactic, so it
        // matches the whitespace *before* the `;` as well, and therefore does
        // not begin where its literal does.
        sep -> () = ";" -> { () }
        SEP -> () = ";" -> { () }

        pub syntactic -> &'a str = s:until(sep) sep digit1 -> { s }
        pub lexical -> &'a str = s:until(SEP) SEP digit1 -> { s }
    }
}

#[test]
fn a_syntactic_rule_terminates_where_its_whitespace_starts() {
    // The whole point of excluding it: `until(sep)` stops before the spaces,
    // because that is where `sep` matches. Scanning for ";" would stop after
    // them, and quietly return two more characters.
    Whitespace::parse_syntactic()
        .parse_test("Hamburg  ;12")
        .assert_success_is("Hamburg");

    // The lexical twin stops at the literal, and keeps the spaces - a
    // different answer for the same input, which is why the two must not be
    // compiled the same way.
    Whitespace::parse_lexical()
        .parse_test("Hamburg  ;12")
        .assert_success_is("Hamburg  ");
}

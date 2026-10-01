//! A character loop - `(not(x) any)*` - is `until(x)` spelled out, and the
//! code generator turns it into the same scan.
//!
//! The tests are about what that must not change: the text matched, where the
//! loop stops, what `+` demands, a value that is collected rather than taken,
//! and what an error at the end of the input says.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Loop {
        // One literal: `until(";")`.
        pub FIELD -> String = s:text((not(";") any)*) ";" -> { s.to_string() }

        // Several lookaheads in a row, as a string body is usually written.
        pub BODY -> String = "\"" s:text((not("\"") not("\\") any)*) "\"" -> { s.to_string() }

        // The same set, written as one lookahead over a group.
        pub GROUPED -> String = s:text((not(("\"" | "\\")) any)*) t:until(eof) -> { format!("{s}|{t}") }

        // A multi-character terminator.
        pub COMMENT -> String = "<!--" s:text((not("-->") any)*) "-->" -> { s.to_string() }

        // `line_ending` stops before the `\r` of a `\r\n`, not before a bare one.
        pub LINE -> String = s:text((not(line_ending) any)*) t:until(eof) -> { format!("{s}|{t}") }

        // Discarded: the loop yields nothing and must still consume.
        pub SKIPPED -> u32 = "#" (not("\n") any)* "\n" n:u32 -> { n }

        // `+` needs one character before the terminator.
        pub NONEMPTY -> String = s:text((not(";") any)+) t:until(eof) -> { format!("{s}|{t}") }

        // A rule of the grammar's own that is nothing but a literal.
        SEP -> () = ";"
        pub BY_RULE -> String = s:text((not(SEP) any)*) SEP -> { s.to_string() }

        // Four needles: more than the scan takes, so the loop stays a loop.
        pub FOUR -> String = s:text((not("a") not("b") not("c") not("d") any)*) t:until(eof) -> { format!("{s}|{t}") }

        // Bound, the loop collects its elements as before.
        pub COLLECTED -> usize = v:(not(";") any)* ";" -> { v.len() }

        // `not` over something that is not a fixed string: still a loop.
        pub TO_DIGIT -> String = s:text((not(digit) any)*) t:until(eof) -> { format!("{s}|{t}") }

        // `dec(…)` reads the scanned text.
        pub NUMBER -> u32 = n:dec<u32>((not(";") any)+) ";" -> { n }

        // An unterminated string.
        pub STR -> () = "\"" (not("\"") any)* "\"" -> { () }
    }
}

#[test]
fn yields_the_text_before_the_terminator() {
    Loop::parse_FIELD()
        .parse_test("Hamburg;")
        .assert_success_is("Hamburg".to_string());
    Loop::parse_FIELD()
        .parse_test(";")
        .assert_success_is(String::new());
}

#[test]
fn stops_at_the_first_of_several_lookaheads() {
    Loop::parse_BODY()
        .parse_test("\"abc\"")
        .assert_success_is("abc".to_string());
    // Stops at the backslash, so the closing quote is not where it is expected.
    assert!(Loop::parse_BODY().parse_test("\"a\\b\"").inner.is_err());
    Loop::parse_GROUPED()
        .parse_test("ab\\c")
        .assert_success_is("ab|\\c".to_string());
}

#[test]
fn scans_for_a_multi_character_terminator() {
    Loop::parse_COMMENT()
        .parse_test("<!-- a - b -- c -->")
        .assert_success_is(" a - b -- c ".to_string());
}

#[test]
fn line_ending_keeps_its_carriage_return() {
    Loop::parse_LINE()
        .parse_test("a\rb\r\nc")
        .assert_success_is("a\rb|\r\nc".to_string());
    Loop::parse_LINE()
        .parse_test("no newline")
        .assert_success_is("no newline|".to_string());
}

#[test]
fn a_discarded_loop_still_consumes() {
    Loop::parse_SKIPPED()
        .parse_test("# a comment\n42")
        .assert_success_is(42);
}

#[test]
fn plus_needs_one_character() {
    Loop::parse_NONEMPTY()
        .parse_test("ab;")
        .assert_success_is("ab|;".to_string());
    Loop::parse_NONEMPTY()
        .parse_test("ab")
        .assert_success_is("ab|".to_string());
    assert!(Loop::parse_NONEMPTY().parse_test(";").inner.is_err());
    assert!(Loop::parse_NONEMPTY().parse_test("").inner.is_err());
}

#[test]
fn a_literal_rule_is_its_literal() {
    Loop::parse_BY_RULE()
        .parse_test("x y;")
        .assert_success_is("x y".to_string());
}

#[test]
fn more_needles_than_the_scan_takes() {
    Loop::parse_FOUR()
        .parse_test("xyzc")
        .assert_success_is("xyz|c".to_string());
}

#[test]
fn a_bound_loop_collects() {
    Loop::parse_COLLECTED()
        .parse_test("Grüße;")
        .assert_success_is(5);
}

#[test]
fn a_terminator_that_is_not_a_string() {
    Loop::parse_TO_DIGIT()
        .parse_test("ab3")
        .assert_success_is("ab|3".to_string());
}

#[test]
fn stops_at_a_multi_byte_character_boundary() {
    Loop::parse_FIELD()
        .parse_test("Grüße 東京;")
        .assert_success_is("Grüße 東京".to_string());
}

#[test]
fn dec_reads_the_scanned_text() {
    Loop::parse_NUMBER()
        .parse_test("1234;")
        .assert_success_is(1234);
    assert!(Loop::parse_NUMBER().parse_test("12x;").inner.is_err());
}

#[test]
fn an_unterminated_string_asks_for_the_quote() {
    let e = match Loop::parse_STR().parse_test("\"abc").inner {
        Ok(v) => panic!("unexpectedly succeeded: {v:?}"),
        Err(e) => e,
    };
    // Not "or any character": the loop's last attempt is not an expectation.
    assert!(
        e.starts_with("unexpected end of input, expected `\"` at"),
        "{e}"
    );
}

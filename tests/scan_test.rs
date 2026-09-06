//! `until(…)` and `recover(…)` scan for a fixed terminator instead of running
//! the terminator's parser once per character.
//!
//! The tests below are about what the scan must not change: the value, the
//! behaviour at end of input, `\r\n`, and UTF-8 boundaries — the scan works in
//! byte offsets, so a multi-byte character before the terminator is the case
//! that would break first.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Scan {
        // A literal terminator: scanned for.
        pub FIELD -> String = s:until(";") ";" -> { s.to_string() }

        // Not consumed, so the terminator is still there for what follows.
        pub REST -> String = until(";") ";" tail:until(";") -> { tail.to_string() }

        // No terminator in sight: consume to the end.
        pub OPEN -> String = s:until(";") -> { s.to_string() }

        // A multi-character terminator.
        pub COMMENT -> String = "<!--" s:until("-->") "-->" -> { s.to_string() }

        // A terminator that is not a fixed string: tried position by position.
        // It must yield the same thing as the scanned paths - the skipped text.
        pub TO_DIGIT -> String = s:until(digit) d:digit -> { format!("{s}|{d}") }
    }
}

#[test]
fn yields_the_text_it_skipped() {
    Scan::parse_FIELD()
        .parse_test("Hamburg;")
        .assert_success_is("Hamburg".to_string());
}

#[test]
fn does_not_consume_the_terminator() {
    // If `until` had eaten the `;`, the explicit `";"` after it would fail.
    Scan::parse_REST()
        .parse_test("abc;def")
        .assert_success_is("def".to_string());
}

#[test]
fn consumes_to_the_end_when_the_terminator_is_absent() {
    Scan::parse_OPEN()
        .parse_test("no terminator here")
        .assert_success_is("no terminator here".to_string());
}

#[test]
fn empty_prefix_is_fine() {
    Scan::parse_FIELD()
        .parse_test(";")
        .assert_success_is(String::new());
}

#[test]
fn scans_for_a_multi_character_terminator() {
    Scan::parse_COMMENT()
        .parse_test("<!-- a - b -- c -->")
        .assert_success_is(" a - b -- c ".to_string());
}

#[test]
fn stops_at_a_multi_byte_character_boundary() {
    // The scan works in byte offsets: a name with non-ASCII text before the
    // terminator must come back whole.
    Scan::parse_FIELD()
        .parse_test("Grüße 東京;")
        .assert_success_is("Grüße 東京".to_string());
}

#[test]
fn a_non_literal_terminator_yields_the_same_value() {
    Scan::parse_TO_DIGIT()
        .parse_test("abc4")
        .assert_success_is("abc|4".to_string());
}

grammar! {
    grammar Lines {
        // `line_ending` is two shapes, not one literal - the scan finds the
        // newline and then looks at the byte before it.
        // The trailing `line_ending? until(";")` is only there so the test can
        // assert on a fully consumed input; the value under test is `s`.
        pub LINE -> String = s:until(line_ending) line_ending? until(";") -> { s.to_string() }
    }
}

#[test]
fn stops_before_a_unix_line_ending() {
    Lines::parse_LINE()
        .parse_test("one\ntwo")
        .assert_success_is("one".to_string());
}

#[test]
fn stops_before_a_windows_line_ending() {
    // The line ending is "\r\n", so the carriage return is not part of the line.
    Lines::parse_LINE()
        .parse_test("one\r\ntwo")
        .assert_success_is("one".to_string());
}

#[test]
fn a_bare_carriage_return_is_ordinary_text() {
    Lines::parse_LINE()
        .parse_test("one\rtwo\nthree")
        .assert_success_is("one\rtwo".to_string());
}

#[test]
fn a_windows_line_ending_after_a_unix_one_is_not_found_first() {
    // Scanning for "\r\n" would run past the "\n" on the first line.
    Lines::parse_LINE()
        .parse_test("one\ntwo\r\n")
        .assert_success_is("one".to_string());
}

#[test]
fn a_line_that_ends_at_end_of_input() {
    Lines::parse_LINE()
        .parse_test("only line")
        .assert_success_is("only line".to_string());
}

grammar! {
    grammar Recovery {
        // Recovery skips to the synchronization token. That skip is the part
        // that runs over the broken region.
        pub items -> usize =
            xs:recover(item, ";")* -> { xs.iter().filter(|x| x.is_some()).count() }

        rule item -> i32 = v:i32 ";" -> { v }
    }
}

#[test]
fn recovery_still_recovers() {
    Recovery::parse_items()
        .parse_test("1; oops; 3;")
        .assert_success_is(2);
}

#[test]
fn recovery_skips_a_long_broken_region() {
    // The point of scanning: the skip runs over the whole broken stretch, and
    // it is reached exactly when a file has many errors. This asserts the
    // result at a size where trying the synchronization token at every position
    // was the dominant cost.
    let junk = "x".repeat(400_000);
    let input = format!("1;{junk};2;");

    Recovery::parse_items()
        .parse_test(&input)
        .assert_success_is(2);
}

grammar! {
    grammar Rest {
        // `until(eof)` is "the rest of the input": no search at all.
        pub REST -> String = "=" s:until(eof) -> { s.to_string() }
    }
}

#[test]
fn until_eof_is_the_rest_of_the_input() {
    Rest::parse_REST()
        .parse_test("=everything after, \n lines and all")
        .assert_success_is("everything after, \n lines and all".to_string());
    Rest::parse_REST()
        .parse_test("=")
        .assert_success_is(String::new());
}

grammar! {
    grammar Shadow {
        // A rule of the grammar's own named `line_ending` is that rule, not
        // the built-in: the terminator is `|`, and a real newline is text.
        rule line_ending = "|"
        pub CELL -> String = s:until(line_ending) "|" -> { s.to_string() }
    }
}

#[test]
fn a_user_rule_named_line_ending_is_not_the_built_in() {
    Shadow::parse_CELL()
        .parse_test("one\ntwo|")
        .assert_success_is("one\ntwo".to_string());
}

grammar! {
    grammar LineRecovery {
        // Recovery synchronising on a line ending takes the scanned path too.
        // Lexical rules: the default whitespace of a syntactic rule is
        // `multispace0`, which would swallow the very newlines being
        // synchronised on.
        pub LINES -> usize =
            xs:recover(ENTRY, line_ending)* -> { xs.iter().filter(|x| x.is_some()).count() }

        rule ENTRY -> i32 = v:i32 line_ending -> { v }
    }
}

#[test]
fn recovery_synchronises_on_a_line_ending() {
    // The broken line ends in "\r\n": the scan must stop before the "\r" so
    // that the synchronisation token still matches whole.
    LineRecovery::parse_LINES()
        .parse_test("1\nbroken line\r\n3\n")
        .assert_success_is(2);
}

#[test]
fn recovery_fails_when_the_synchronization_token_never_comes() {
    // The skip runs to the end of the input; the synchronization token is
    // then missing, and that is a failure - not a hang, not a panic.
    Recovery::parse_items()
        .parse_test("1;oops")
        .assert_failure();
}

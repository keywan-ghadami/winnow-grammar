//! The delimiter patterns - `( … )` via `paren(…)`, `[ … ]` and `{ … }`.
//!
//! The braced form generated `]` as its closing token, so every grammar that
//! matched literal braces failed at the closing brace at runtime. Bracketed
//! and parenthesised delimiters were unaffected, which is why it went
//! unnoticed - the three forms share one code path and differ only in the two
//! strings passed into it.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Delims {
        pub braced -> String = "b" { name:raw_ident } -> { name.to_string() }
        pub bracketed -> String = "b" [ name:raw_ident ] -> { name.to_string() }
        pub parenthesised -> String = "b" paren(name:raw_ident) -> { name.to_string() }

        // Not the last element of the sequence: the closing brace has to be
        // consumed for what follows to see the right position.
        pub braced_then -> String = { name:raw_ident } "!" -> { name.to_string() }

        // A lexical rule takes the same path without the whitespace skipping.
        pub BRACED -> String = "b" { name:raw_ident } -> { name.to_string() }
    }
}

#[test]
fn a_braced_delimiter_matches_both_braces() {
    Delims::parse_braced()
        .parse_test("b { hello }")
        .assert_success_is("hello".to_string());
}

#[test]
fn a_braced_delimiter_needs_its_closing_brace() {
    Delims::parse_braced()
        .parse_test("b { hello")
        .assert_failure();
    // `]` is not a closing brace - the regression this file exists for.
    Delims::parse_braced()
        .parse_test("b { hello ]")
        .assert_failure();
}

#[test]
fn a_braced_delimiter_is_not_the_end_of_the_sequence() {
    Delims::parse_braced_then()
        .parse_test("{ hello } !")
        .assert_success_is("hello".to_string());
}

#[test]
fn a_braced_delimiter_works_in_a_lexical_rule() {
    Delims::parse_BRACED()
        .parse_test("b{hello}")
        .assert_success_is("hello".to_string());
}

#[test]
fn the_other_delimiters_still_match() {
    Delims::parse_bracketed()
        .parse_test("b [ hello ]")
        .assert_success_is("hello".to_string());
    Delims::parse_parenthesised()
        .parse_test("b ( hello )")
        .assert_success_is("hello".to_string());
}

grammar! {
    grammar Keyword {
        // `brace(…)` is the keyword form of `{ … }`, for the one content the
        // bare form cannot express: a leading integer literal, which reads as
        // a repetition bound. `paren(…)` plays the same role for `( … )`.
        pub kw_braced -> String = "b" brace(name:raw_ident) -> { name.to_string() }
    }
}

#[test]
fn the_keyword_form_of_a_braced_delimiter_matches_braces() {
    Keyword::parse_kw_braced()
        .parse_test("b { hello }")
        .assert_success_is("hello".to_string());
    Keyword::parse_kw_braced()
        .parse_test("b ( hello )")
        .assert_failure();
}

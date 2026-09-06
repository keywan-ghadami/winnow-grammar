//! `count(pattern)` - the number of matches instead of the matches.
//!
//! Documented in SYNTAX.md but never exercised: bound to a name it was
//! generated as `let n: Vec<_> = …` around a parser that had already mapped
//! to `usize`, and in a lexical rule the generated path was not even
//! syntactically valid Rust. Neither shape compiled.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Counting {
        // Lexical: no whitespace skipping between the counted elements.
        pub DIGITS -> usize = n:count(digit) -> { n }

        // Syntactic: whitespace between the elements is skipped as usual.
        pub items -> usize = n:count(raw_ident) -> { n }

        // Unbound, next to a binding that carries the result.
        pub tagged -> String = count(digit) name:raw_ident -> { name.to_string() }
    }
}

#[test]
fn a_lexical_count_counts_characters() {
    Counting::parse_DIGITS()
        .parse_test("123")
        .assert_success_is(3);
    Counting::parse_DIGITS().parse_test("").assert_success_is(0);
}

#[test]
fn a_syntactic_count_counts_elements() {
    Counting::parse_items()
        .parse_test("a bb ccc")
        .assert_success_is(3);
}

#[test]
fn an_unbound_count_is_discarded() {
    Counting::parse_tagged()
        .parse_test("12 x")
        .assert_success_is("x".to_string());
}

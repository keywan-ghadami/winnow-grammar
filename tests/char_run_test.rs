//! A repetition of a single character is a run, and a run is text.
//!
//! `{n,m}` is borrowed from regular expressions, where `\d{1,2}` matches
//! *text*. Yielding a `Vec<char>` there copies characters that are already in
//! the input, and no grammar wanted that copy: every use in this repository
//! turned it straight back into a string, a count or a number.
//!
//! So a repetition whose element is a character *class* - `digit`, `any` -
//! yields `&'a str`. Every other repetition still yields its elements,
//! because those are values the parser built rather than input it walked
//! over. `char` is not a class although it yields a `char`: it parses a
//! character literal, and `'\n'` is four characters of input and one of
//! value.

use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, Symbol};

grammar! {
    grammar Runs {
        // Bounded, open-ended and unbounded runs of digits: all text.
        pub BOUNDED -> &'a str = d:digit{1,2} -> { d }
        pub EXACT -> &'a str = d:digit{3} -> { d }
        pub OPEN -> &'a str = d:digit{2,} -> { d }
        pub STAR -> &'a str = d:digit* -> { d }
        pub PLUS -> &'a str = d:digit+ -> { d }

        // `any` is the other character class. `char` is deliberately not one:
        // it parses a character *literal*, so `'\n'` is four characters of
        // input and one of value - its text is not its value.
        pub ANY -> &'a str = a:any{3} -> { a }
        pub LITERALS -> Vec<char> = cs:char{2} -> { cs }

        // A run is a `&str`, so it interns without `text(..)` in the way.
        pub CODE -> Symbol = s:intern(digit{3}) -> { s }

        // And reads as a number the same way.
        pub NUMBER -> u32 = n:dec<u32>(digit{1,3}) -> { n }

        // `text(..)` over a run is the same thing, not a second wrapper.
        pub WRAPPED -> &'a str = s:text(digit{1,2}) -> { s }

        // A repetition of anything else still yields its elements: `word`
        // builds a value, so the `Vec` is the answer rather than a copy.
        // Lowercase, so the elements may be separated by whitespace.
        word -> usize = s:raw_ident -> { s.len() }
        pub words -> Vec<usize> = ws:word{1,3} -> { ws }

        // `digit1` is itself a run, so repeating it repeats runs.
        pub RUNS -> usize = rs:digit1{1,2} -> { rs.len() }

        // Nothing names it, so nothing is built either way.
        pub DISCARDED -> () = digit{1,2} -> { () }
    }
}

#[test]
fn a_run_of_digits_is_text() {
    Runs::parse_BOUNDED()
        .parse_test("12")
        .assert_success_is("12");
    Runs::parse_BOUNDED().parse_test("1").assert_success_is("1");
    Runs::parse_EXACT()
        .parse_test("123")
        .assert_success_is("123");
    Runs::parse_OPEN()
        .parse_test("123456")
        .assert_success_is("123456");
    Runs::parse_PLUS().parse_test("42").assert_success_is("42");
}

/// `digit*` is the `digit0` the built-in table never had: zero digits are a
/// match, and the text is empty.
#[test]
fn a_star_run_matches_nothing_and_yields_nothing() {
    Runs::parse_STAR().parse_test("").assert_success_is("");
    Runs::parse_STAR().parse_test("77").assert_success_is("77");
}

#[test]
fn any_is_a_character_class_and_char_is_not() {
    Runs::parse_ANY()
        .parse_test("x!\n")
        .assert_success_is("x!\n");
    // `char{2}` still collects: two literals, two values, and `'\n'` is one
    // of them rather than the two characters that spell it.
    Runs::parse_LITERALS()
        .parse_test("'a''\\n'")
        .assert_success_is(vec!['a', '\n']);
}

#[test]
fn a_run_interns_and_reads_as_a_number_without_a_wrapper() {
    Runs::parse_CODE()
        .parse_test("123")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "123"));
    Runs::parse_NUMBER()
        .parse_test("407")
        .assert_success_is(407u32);
    Runs::parse_WRAPPED()
        .parse_test("42")
        .assert_success_is("42");
}

/// The exception is exactly as wide as its reason: a repetition of anything
/// that is not a single character still collects.
#[test]
fn a_repetition_of_values_still_yields_its_elements() {
    Runs::parse_words()
        .parse_test("ab cde f")
        .assert_success_is(vec![2usize, 3, 1]);
    // `digit1` yields a run, so repeating it yields runs - two of them here,
    // because the first is greedy and the second needs the rest.
    Runs::parse_RUNS()
        .parse_test("12")
        .assert_success_is(1usize);
}

#[test]
fn the_bound_still_holds() {
    Runs::parse_EXACT().parse_test("12").assert_failure();
    Runs::parse_OPEN().parse_test("1").assert_failure();
    // Greedy and possessive, unchanged: `{1,2}` takes two and leaves the rest.
    Runs::parse_BOUNDED().parse_test("123").assert_failure();
    Runs::parse_DISCARDED().parse_test("12").assert_success();
}

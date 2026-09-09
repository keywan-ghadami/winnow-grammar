//! `text(p)`: what was matched, not what was parsed.
//!
//! A repetition of a rule yields its elements, as a `Vec` - right when the
//! elements are what you want and a heap allocation when they are not.
//! `text(p)` runs `p` and hands back the input it consumed, borrowed from the
//! input itself: `&'a str`, no allocation, whatever `p` is.
//!
//! Over a run of a character class it is a no-op - `digit{1,2}` already yields
//! its text (`tests/char_run_test.rs`), and codegen emits the run itself
//! rather than a second `take` around it. The cases here are the ones where
//! `p` is something else: a sequence, an alternative, an optional.

use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, Symbol};

grammar! {
    grammar Text {
        // A bounded run of digits as the text it matched.
        pub TENTHS -> i32 =
            neg:"-"? whole:text(digit{1,2}) "." frac:digit
            -> {
                let mut v: i32 = 0;
                for &b in whole.as_bytes() { v = v * 10 + (b - b'0') as i32; }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // `text` is not about digits: it takes any pattern.
        pub PAIR -> &'a str = s:text("a" "b" "c") -> { s }
        pub OPTIONAL -> &'a str = s:text("x"? "y") -> { s }
        pub ALTERNATIVE -> &'a str = s:text(("ab" | "cd")) -> { s }
        pub NESTED -> &'a str = s:text(digit{2} raw_ident) -> { s }

        // What it does not do: consume anything of its own. The terminator is
        // still there for the next element.
        pub STOPS -> (&'a str, &'a str) = a:text(digit1) rest:raw_ident -> { (a, rest) }

        // The gap it closes: a bounded run can now be interned.
        pub CODE -> Symbol = s:intern(text(digit{3})) -> { s }

        // Zero-length matches are text too.
        pub EMPTY -> &'a str = s:text("a"?) -> { s }
    }
}

#[test]
fn a_bounded_digit_run_is_its_text() {
    Text::parse_TENTHS()
        .parse_test("-12.3")
        .assert_success_is(-123);
    Text::parse_TENTHS().parse_test("4.5").assert_success_is(45);
    Text::parse_TENTHS()
        .parse_test("-0.1")
        .assert_success_is(-1);
}

#[test]
fn text_takes_any_pattern_not_only_digits() {
    Text::parse_PAIR()
        .parse_test("abc")
        .assert_success_is("abc");
    Text::parse_OPTIONAL()
        .parse_test("xy")
        .assert_success_is("xy");
    Text::parse_OPTIONAL()
        .parse_test("y")
        .assert_success_is("y");
    Text::parse_ALTERNATIVE()
        .parse_test("cd")
        .assert_success_is("cd");
    Text::parse_NESTED()
        .parse_test("12ab")
        .assert_success_is("12ab");
}

#[test]
fn text_consumes_exactly_what_its_pattern_consumes() {
    Text::parse_STOPS()
        .parse_test("12ab")
        .assert_success_is(("12", "ab"));
    // An empty match yields the empty slice, not a failure.
    Text::parse_EMPTY().parse_test("").assert_success_is("");
}

#[test]
fn a_bounded_run_can_now_be_interned() {
    Text::parse_CODE()
        .parse_test("123")
        .assert_success_with(|s, ctx| assert_eq!(ctx.interner.resolve(*s), "123"));
}

/// The bound still holds: `text` reports what its pattern reports.
#[test]
fn a_failing_pattern_fails_the_text() {
    Text::parse_TENTHS().parse_test("123.4").assert_failure();
    Text::parse_NESTED().parse_test("1ab").assert_failure();
}

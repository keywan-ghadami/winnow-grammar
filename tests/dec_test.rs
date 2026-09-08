//! `dec<T>(p)`: the digits `p` matched, as a number.
//!
//! Not for speed - measured, it is the same as `text(p)` plus the fold an
//! action would write by hand (`benches/repetition.rs`, and TODO.md §5 has
//! the runs). It is here for the two things that fold does badly: it is
//! written out again at every numeric field, and it silently overflows if the
//! author picks a type the format does not fit.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Dec {
        // The 1BRC temperature, with no arithmetic left in the action but the
        // one that combines the parts.
        pub TENTHS -> i32 =
            neg:"-"? whole:dec<i32>(digit{1,2}) "." frac:dec<i32>(digit)
            -> { let v = whole * 10 + frac; if neg.is_some() { -v } else { v } }

        // The type is stated, not guessed from the bound.
        pub SMALL -> u8 = n:dec<u8>(digit{1,3}) -> { n }
        pub BIG -> u64 = n:dec<u64>(digit1) -> { n }

        // Any pattern whose text is digits, not only a bounded run.
        pub RUN -> u32 = n:dec<u32>(digit1) -> { n }
        pub EXACT -> u32 = n:dec<u32>(digit{4}) -> { n }

        // It stops where its pattern stops.
        pub STOPS -> (u32, &'a str) = n:dec<u32>(digit{2}) rest:raw_ident -> { (n, rest) }
    }
}

#[test]
fn a_bounded_run_becomes_a_number() {
    Dec::parse_TENTHS()
        .parse_test("-12.3")
        .assert_success_is(-123);
    Dec::parse_TENTHS().parse_test("4.5").assert_success_is(45);
    Dec::parse_TENTHS().parse_test("-0.1").assert_success_is(-1);
}

#[test]
fn the_type_is_the_one_the_grammar_names() {
    Dec::parse_SMALL()
        .parse_test("255")
        .assert_success_is(255u8);
    Dec::parse_BIG()
        .parse_test("18446744073709551615")
        .assert_success_is(u64::MAX);
    Dec::parse_RUN()
        .parse_test("1234567")
        .assert_success_is(1234567u32);
    Dec::parse_EXACT()
        .parse_test("0042")
        .assert_success_is(42u32);
}

#[test]
fn it_consumes_exactly_what_its_pattern_consumes() {
    Dec::parse_STOPS()
        .parse_test("12ab")
        .assert_success_is((12u32, "ab"));
}

/// A value the named type cannot hold is a parse failure, not a wrapped
/// number. This is the whole reason to prefer it over a fold in an action:
/// there, `v * 10 + d` on a `u8` wraps in release and panics in debug.
#[test]
fn a_value_too_large_for_its_type_fails_the_parse() {
    Dec::parse_SMALL()
        .parse_test("256")
        .assert_failure_contains("number too large");
    Dec::parse_BIG()
        .parse_test("18446744073709551616")
        .assert_failure_contains("number too large");
}

/// The pattern's own failure is still the pattern's.
#[test]
fn a_failing_pattern_fails_the_number() {
    Dec::parse_EXACT().parse_test("12").assert_failure();
    Dec::parse_TENTHS().parse_test("123.4").assert_failure();
}

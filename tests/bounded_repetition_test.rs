//! `p{n}` / `p{n,}` / `p{n,m}` — repetition with explicit bounds.
//!
//! The motivation is fixed-width data: a format that states "one or two digits,
//! then a point, then exactly one" can be parsed without asking how far the
//! digits run. `digit+` says *unbounded*, and a parser that assumed otherwise
//! would be inventing a constraint the grammar never stated.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Bounds {
        // Exactly two digits, then whatever follows.
        pub PAIR -> String = d:digit{2} rest:raw_ident -> {
            format!("{}{}-{}", d[0], d[1], rest)
        }

        // One or two digits.
        pub SHORT -> String = d:digit{1,2} -> { d.iter().collect() }

        // Two or more.
        pub OPEN -> String = d:digit{2,} -> { d.iter().collect() }
    }
}

#[test]
fn exact_count_matches() {
    Bounds::parse_PAIR()
        .parse_test("42abc")
        .assert_success_is("42-abc".to_string());
}

#[test]
fn exact_count_stops_at_the_bound() {
    // Three digits: the repetition takes two and leaves the third to the
    // pattern after it. A greedy `digit+` would have swallowed all three.
    Bounds::parse_PAIR()
        .parse_test("12x")
        .assert_success_is("12-x".to_string());
}

#[test]
fn exact_count_fails_below_the_bound() {
    Bounds::parse_PAIR().parse_test("4abc").assert_failure();
}

#[test]
fn range_takes_as_many_as_it_can() {
    Bounds::parse_SHORT()
        .parse_test("42")
        .assert_success_is("42".to_string());
}

#[test]
fn range_accepts_the_lower_bound() {
    Bounds::parse_SHORT()
        .parse_test("4")
        .assert_success_is("4".to_string());
}

#[test]
fn range_stops_at_the_upper_bound() {
    // "123" against `digit{1,2}`: two digits are taken, and the leftover "3"
    // makes the *parse* fail rather than the repetition grow.
    Bounds::parse_SHORT().parse_test("123").assert_failure();
}

#[test]
fn range_fails_below_the_lower_bound() {
    Bounds::parse_SHORT().parse_test("x").assert_failure();
}

#[test]
fn open_upper_bound_is_unlimited() {
    Bounds::parse_OPEN()
        .parse_test("1234567")
        .assert_success_is("1234567".to_string());
}

#[test]
fn open_upper_bound_still_has_a_minimum() {
    Bounds::parse_OPEN().parse_test("1").assert_failure();
}

grammar! {
    grammar Measurement {
        // The 1BRC temperature: -99.9 to 99.9, always one decimal. Parsed as
        // tenths so the aggregation stays in integer arithmetic.
        pub TENTHS -> i32 =
            neg:"-"? whole:digit{1,2} "." frac:digit
            -> {
                let mut value: i32 = 0;
                for d in whole { value = value * 10 + (d as i32 - '0' as i32); }
                value = value * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -value } else { value }
            }
    }
}

#[test]
fn parses_a_fixed_width_temperature() {
    Measurement::parse_TENTHS()
        .parse_test("12.3")
        .assert_success_is(123);
    Measurement::parse_TENTHS()
        .parse_test("-4.5")
        .assert_success_is(-45);
    Measurement::parse_TENTHS()
        .parse_test("99.9")
        .assert_success_is(999);
}

#[test]
fn rejects_a_temperature_that_is_too_wide() {
    // Three whole digits are outside the stated format.
    Measurement::parse_TENTHS()
        .parse_test("123.4")
        .assert_failure();
}

grammar! {
    grammar Braces {
        // A brace group is still the braced-delimiter pattern: only one whose
        // content *starts with an integer* is read as a repetition bound. That
        // this grammar compiles is the assertion — had `{ name:raw_ident }`
        // been read as a bound, the bound parser would have rejected it at
        // macro-expansion time and this file would not build.
        pub block -> String = "b" { name:raw_ident } -> { name.to_string() }
    }
}

#[test]
fn a_braced_pattern_is_not_a_bound() {
    // Naming the parser is enough; see the comment above. (The braced
    // delimiter's own runtime behaviour is unrelated to bounds.)
    let _ = Braces::parse_block::<()>;
}

grammar! {
    grammar IntInBraces {
        // The documented cost of the disambiguation: a brace group holding a
        // bare integer is a bound, not a delimiter pattern. `"b"{2}` matches
        // two `b`s. Braces around a literal `2` are written `"{" "2" "}"`.
        pub twice -> usize = b:"b"{2} -> { b.len() }
    }
}

#[test]
fn a_brace_group_holding_an_integer_is_a_bound() {
    IntInBraces::parse_twice()
        .parse_test("bb")
        .assert_success_is(2);
    IntInBraces::parse_twice().parse_test("b").assert_failure();
}

grammar! {
    grammar Spaced {
        // In a syntactic (lowercase) rule the bound counts elements, and
        // whitespace between them is skipped as usual.
        pub three -> usize = n:i32{3} -> { n.len() }
    }
}

#[test]
fn bounds_count_elements_not_characters() {
    Spaced::parse_three()
        .parse_test("1 22 333")
        .assert_success_is(3);
}

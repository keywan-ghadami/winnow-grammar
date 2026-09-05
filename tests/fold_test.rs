//! `fold(pattern, init, step)` — a repetition that threads an accumulator
//! instead of collecting into a `Vec`.

use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar Sum {
        // Sum the numbers without ever building a Vec of them.
        pub total -> i64 = t:fold(i64, || 0i64, |acc: i64, n: i64| acc + n) -> { t }
    }
}

#[test]
fn folds_a_sequence() {
    Sum::parse_total()
        .parse_test("1 2 3 4")
        .assert_success_with(|sum, _| assert_eq!(*sum, 10));
}

#[test]
fn empty_input_yields_the_initial_accumulator() {
    // A fold is a zero-or-more repetition, so it succeeds on empty input.
    Sum::parse_total()
        .parse_test("")
        .assert_success_with(|sum, _| assert_eq!(*sum, 0));
}

grammar! {
    grammar Records {
        // A line-oriented format: "name=value" per line, folded into a count
        // and a running total — the shape a large data file needs.
        pub summary -> (usize, i64) =
            s:fold(record, || (0usize, 0i64), |acc: (usize, i64), r: i64| (acc.0 + 1, acc.1 + r))
            -> { s }

        rule record -> i64 = ident "=" v:i64 -> { v }
    }
}

#[test]
fn folds_structured_records() {
    Records::parse_summary()
        .parse_test("a=1 b=2 c=39")
        .assert_success_with(|&(count, total), _| {
            assert_eq!(count, 3);
            assert_eq!(total, 42);
        });
}

#[test]
fn folds_a_large_input_in_constant_space() {
    // The point of the feature: 200_000 records must not materialise as a
    // collection. This asserts the result, not the memory — but it does run the
    // path at a size where collecting would be the dominant cost.
    let input = (0..200_000)
        .map(|i| format!("r{i}={}", i % 7))
        .collect::<Vec<_>>()
        .join(" ");

    let expected: i64 = (0..200_000i64).map(|i| i % 7).sum();

    Records::parse_summary()
        .parse_test(&input)
        .assert_success_with(|&(count, total), _| {
            assert_eq!(count, 200_000);
            assert_eq!(total, expected);
        });
}

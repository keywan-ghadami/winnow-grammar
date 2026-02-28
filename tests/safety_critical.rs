use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;
use winnow_grammar::testing::Testable;

// 1. Cut Operator Safety: ensuring that once we commit to a path, we do NOT backtrack.
// This is critical for preventing ambiguity and ensuring deterministic parsing.
grammar! {
    grammar CutSafety {
        // A rule that uses cut. If "commit" is found, we MUST match "success".
        // If "success" fails, we should NOT backtrack to the second alternative.
        pub rule deterministic_choice -> &'static str =
            "commit" => "success" -> { "committed" }
          | "commit" "failure"    -> { "backtracked_badly" }
          | "other"               -> { "other" }
    }
}

#[test]
fn test_cut_operator_safety() {
    // Scenario 1: Successful commit
    let input = LocatingSlice::new("commitsuccess");
    CutSafety::parse_deterministic_choice
        .parse(input)
        .test()
        .assert_success_is("committed");

    // Scenario 2: Failure after commit
    // Input is "commitfail".
    // "commit" matches. Cut `=>` executes.
    // "success" fails.
    // Because of cut, we must NOT try the second alternative "commit" "failure".
    // The parser should fail immediately.
    let input = LocatingSlice::new("commitfailure");
    CutSafety::parse_deterministic_choice
        .parse(input)
        .test()
        .assert_failure_contains("success"); // We expect it to be looking for "success"

    // Scenario 3: Alternative path
    let input = LocatingSlice::new("other");
    CutSafety::parse_deterministic_choice
        .parse(input)
        .test()
        .assert_success_is("other");
}

// 2. Strict Error Propagation
// In safety-critical systems, we need to know exactly where parsing failed.
grammar! {
    grammar ErrorProp {
        pub rule main -> () =
            "start" => inner_rule -> { () }

        rule inner_rule -> () =
            "expecting_this" -> { () }
    }
}

#[test]
fn test_error_propagation() {
    let input = LocatingSlice::new("start wrong");

    // We verify that the error is propagated correctly and contains relevant info.
    // winnow's default error messages for literals usually include what was expected.
    ErrorProp::parse_main
        .parse(input)
        .test()
        .assert_failure_contains("expecting_this");
}

// 3. Recursive Robustness (Stack Safety)
// While true stack safety depends on the environment, we verify the grammar
// handles deep nesting without logic errors (up to standard stack limits).
grammar! {
    grammar DeepRecursion {
        pub rule recursive -> usize =
            "(" r:recursive ")" -> { r + 1 }
          | "end"               -> { 0 }
    }
}

#[test]
fn test_deep_recursion() {
    // Construct deeply nested input: (((...(end)...)))
    let depth = 500;
    let mut input = String::new();
    for _ in 0..depth {
        input.push('(');
    }
    input.push_str("end");
    for _ in 0..depth {
        input.push(')');
    }

    DeepRecursion::parse_recursive
        .parse(LocatingSlice::new(input.as_str()))
        .test()
        .assert_success_is(depth);
}

// 4. Input Boundary / Edge Cases
grammar! {
    grammar Boundaries {
        pub rule primitive_limits -> (u8, i8, u128) =
            u:u8 i:i8 huge:u128 -> { (u, i, huge) }
    }
}

#[test]
fn test_numeric_boundaries() {
    // Test max limits
    let input = LocatingSlice::new("255 127 340282366920938463463374607431768211455");
    Boundaries::parse_primitive_limits
        .parse(input)
        .test()
        .assert_success_is((255, 127, u128::MAX));

    // Test overflow behavior (should fail safely, not panic)
    let input = LocatingSlice::new("256 0 0");
    Boundaries::parse_primitive_limits
        .parse(input)
        .test()
        .assert_failure();
}

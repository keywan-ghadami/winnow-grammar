use winnow::prelude::*;
use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

// 1. Cut Operator Safety: ensuring that once we commit to a path, we do NOT backtrack.
// This is critical for preventing ambiguity and ensuring deterministic parsing.
grammar! {
    grammar CutSafety {
        // A rule that uses cut. If "commit" is found, we MUST match "success".
        // If "success" fails, we should NOT backtrack to the second alternative.
        pub rule deterministic_choice -> &'input str =
            "commit" => "success" -> { "committed" }
          | "commit" "failure"    -> { "backtracked_badly" }
          | "other"               -> { "other" }
    }
}

#[test]
fn test_cut_operator_safety() {
    // Scenario 1: Successful commit
    CutSafety::parse_deterministic_choice()
        .parse_test("commitsuccess")
        .assert_success_is("committed");

    // Scenario 2: Failure after commit
    // Input is "commitfail".
    // "commit" matches. Cut `=>` executes.
    // "success" fails.
    // Because of cut, we must NOT try the second alternative "commit" "failure".
    // The parser should fail immediately.
    CutSafety::parse_deterministic_choice()
        .parse_test("commitfailure")
        .assert_failure_contains("success"); // We expect it to be looking for "success"

    // Scenario 3: Alternative path
    CutSafety::parse_deterministic_choice()
        .parse_test("other")
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
    // We verify that the error is propagated correctly and contains relevant info.
    // winnow's default error messages for literals usually include what was expected.
    ErrorProp::parse_main()
        .parse_test("start wrong")
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

    DeepRecursion::parse_recursive()
        .parse_test(&input)
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
    Boundaries::parse_primitive_limits()
        .parse_test("255 127 340282366920938463463374607431768211455")
        .assert_success_is((255, 127, u128::MAX));

    // Test overflow behavior (should fail safely, not panic)
    Boundaries::parse_primitive_limits()
        .parse_test("256 0 0")
        .assert_failure();
}

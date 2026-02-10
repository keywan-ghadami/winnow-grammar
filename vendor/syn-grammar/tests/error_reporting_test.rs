use syn::parse::Parser;
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

#[test]
fn test_deepest_error_wins() {
    grammar! {
        grammar error_test {
            rule main -> String =
                // Try 'deep' first, then 'shallow'.
                // If 'deep' fails after consuming tokens, we want THAT error.
                a:deep_rule -> { a }
                |
                b:shallow_rule -> { b }

            rule deep_rule -> String =
                "start" "deep" "target" -> { "ok".to_string() }

            rule shallow_rule -> String =
                "start" "other" -> { "ok".to_string() }
        }
    }

    // Input: "start deep wrong"
    // 1. deep_rule: Matches "start", matches "deep", fails at "wrong" (Expected "target").
    //    This is a DEEP error (progress made).
    // 2. shallow_rule: Matches "start", fails at "deep" (Expected "other").
    //    This is a DEEP error too, but usually we want the one that went further.
    //    However, let's look at a clearer case.

    // Input: "start wrong"
    // 1. deep_rule: Matches "start", fails at "wrong" (Expected "deep").
    //    Progress: 1 token.
    // 2. shallow_rule: Matches "start", fails at "wrong" (Expected "other").
    //    Progress: 1 token.
    // This is ambiguous.

    // Let's try a case where one is clearly deeper.
    grammar! {
        grammar distinct {
            rule main -> String =
                l:long -> { l }
              | s:short -> { s }

            rule long -> String = "a" "b" "c" -> { "long".into() }
            rule short -> String = "a" "d" -> { "short".into() }
        }
    }

    // Input: "a b x"
    // 'long' matches "a", "b", fails at "x" (Expected "c").
    // 'short' matches "a", fails at "b" (Expected "d").
    // 'long' went further. We expect "expected `c`".

    let err = distinct::parse_main
        .parse_str("a b x")
        .test()
        .assert_failure();

    let msg = err.to_string();
    assert!(
        msg.contains("expected `c`"),
        "Error should mention expected 'c', but got: '{}'",
        msg
    );
    assert!(
        !msg.contains("expected `d`"),
        "Error should NOT mention expected 'd', but got: '{}'",
        msg
    );
}

#[test]
fn test_deep_vs_shallow() {
    grammar! {
        grammar priority {
            rule main -> String =
                d:deep -> { d }
              | s:shallow -> { s }

            // Fails at 2nd token
            rule deep -> String = "x" "y" -> { "y".into() }

            // Fails at 1st token
            rule shallow -> String = "z" -> { "z".into() }
        }
    }

    // Input: "x a"
    // deep: matches "x", fails at "a" (Expected "y"). (Deep error)
    // shallow: fails at "x" (Expected "z"). (Shallow error)
    // We expect "Expected `y`".

    let err = priority::parse_main
        .parse_str("x a")
        .test()
        .assert_failure();

    let msg = err.to_string();
    assert!(
        msg.contains("y"),
        "Should report deep error (expected y), got: '{}'",
        msg
    );
}

#[test]
fn test_rule_name_in_error_message() {
    grammar! {
        grammar rule_context {
            rule main -> String =
                a:inner -> { a }
              | "dummy" -> { "dummy".to_string() }

            // Use alternatives in inner to force it to record its own errors via attempt()
            rule inner -> String =
                "start" "target" -> { "ok".to_string() }
              | "start" "target2" -> { "ok".to_string() }
        }
    }

    // Input: "start wrong"
    // 1. inner attempts var1: fails at "wrong". Records "Error in rule 'inner': expected `target`".
    // 2. inner attempts var2: fails at "wrong". Records "Error in rule 'inner': expected `target2`".
    // 3. inner returns Err.
    // 4. main attempts inner: fails. Records "Error in rule 'main': ...".
    //
    // The errors from (1) and (2) are deeper (at "wrong") than the error from (4) (at "start").
    // So the final error should be one of the inner ones, containing "Error in rule 'inner'".

    let err = rule_context::parse_main
        .parse_str("start wrong")
        .test()
        .assert_failure();

    let msg = err.to_string();
    assert!(
        msg.contains("Error in rule 'inner'"),
        "Expected rule name 'inner' in error, got: {}",
        msg
    );
}

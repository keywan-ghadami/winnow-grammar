use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar WsRepro {
        // Suppress unused warning because 'ws' is used implicitly by the parser generator
        // for whitespace handling, but rustc might not see the direct call.
        #[allow(dead_code)]
        rule ws -> () = multispace0 -> { () }

        pub rule test -> String = "a" -> { "a".to_string() }
        pub rule test_eof -> String = "a" eof -> { "a".to_string() }
    }
}

#[test]
fn test_ws_recursion() {
    WsRepro::parse_test.parse_test("  a").assert_success();
}

#[test]
fn test_eof() {
    WsRepro::parse_test_eof.parse_test("a").assert_success();

    // "a " -> "a" matches. Then implicit ws consumes " ". Then "eof" matches.
    // So this should succeed in syntactic mode.
    WsRepro::parse_test_eof.parse_test("a ").assert_success();

    // "a b" -> "a" matches. ws consumes " ". "b" remains. eof fails.
    WsRepro::parse_test_eof.parse_test("a b").assert_failure();
}
